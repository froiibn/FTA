package app.fta.android;

import android.app.*;
import android.content.*;
import android.content.pm.ServiceInfo;
import android.net.*;
import android.os.*;
import org.json.*;
import java.io.*;
import java.util.*;
import java.util.concurrent.*;

// The Service owns one transport session. No worker, network callback or notification retains an Activity.
public final class TransferService extends Service {
    static final String STATE_PERMISSION="app.fta.android.permission.SERVICE_STATE";
    static final String STATE_CHANGED="app.fta.android.SERVICE_STATE_CHANGED";
    static final ExecutorService CONTROL=Executors.newSingleThreadExecutor();
    private static final ExecutorService WORK=Executors.newFixedThreadPool(3);
    private static final int STATUS_ID=4101,REGISTRATION_ID=4102,POLL_MS=750,IDLE_POLL_MS=3000,EVENT_PAGE_SIZE=1;
    private static final long RECONNECT_GRACE_MS=60000,ACTIVE_WAKE_MS=120000;
    private static final String CHANNEL="connection",STOP="app.fta.android.STOP",START="app.fta.android.START";
    private static volatile TransferService owner;
    private final Handler main=new Handler(Looper.getMainLooper());
    private final Set<String> clients=new HashSet<>();
    private final IBinder binder=new LocalBinder();
    final class LocalBinder extends Binder {TransferService service(){return TransferService.this;}}
    boolean ready,running,starting,foreground,sending;
    String activeId,activePeer,lastError,lastOutcome,lastOutcomeState,lastOutcomeBody;
    boolean lastOutcomeFiles;
    double fraction=Double.NaN;
    JSONArray peers=new JSONArray(),history=new JSONArray(),registrations=new JSONArray();
    final HashMap<String,Completion> completed=new HashMap<>();
    static final class Completion {final String id;final long until;Completion(String value){id=value;until=SystemClock.elapsedRealtime()+1500;}}
    private SharedPreferences preferences;
    private ReceiptAlerts receiptAlerts;
    volatile boolean uiVisible;
    ReceiptAlerts.Notice receiptNotice;
    private ConnectivityManager connectivity;
    private ConnectivityManager.NetworkCallback networkCallback;
    private final BroadcastReceiver idleReceiver=new BroadcastReceiver(){public void onReceive(Context context,Intent intent){updateNotification();}};
    private LanDiscovery discovery;
    private PowerManager.WakeLock activeWake;
    private volatile boolean destroyed;
    private volatile long generation;
    private boolean polling,startRequested,startBlocked,lastNotificationPermission,transferActive;
    private final Runnable pollTick=this::poll;
    private long pollingGeneration=-1,wakeBytes=-1,restartDeadline,offlineDeadline;
    private String desired="",registrationNotice="",notificationText="";
    private final Runnable expireReconnect=()->{if(!running&&!starting)endForeground();};

    @Override public void onCreate(){super.onCreate();owner=this;preferences=getSharedPreferences("fta",MODE_PRIVATE);receiptAlerts=new ReceiptAlerts(this);connectivity=getSystemService(ConnectivityManager.class);discovery=new LanDiscovery(this,CONTROL);discovery.failure=text->lastError=text;
        activeWake=getSystemService(PowerManager.class).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK,"FTA:active-transfer");activeWake.setReferenceCounted(false);
        getSystemService(NotificationManager.class).createNotificationChannel(new NotificationChannel(CHANNEL,getString(R.string.connection_channel),NotificationManager.IMPORTANCE_LOW));
        networkCallback=new ConnectivityManager.NetworkCallback(){public void onAvailable(Network n){changed();}public void onLost(Network n){changed();}public void onCapabilitiesChanged(Network n,NetworkCapabilities c){changed();}public void onLinkPropertiesChanged(Network n,LinkProperties p){changed();}private void changed(){main.post(()->reconcile(false));}};
        connectivity.registerNetworkCallback(new NetworkRequest.Builder().addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN).build(),networkCallback);
        if(Build.VERSION.SDK_INT>=33)registerReceiver(idleReceiver,new IntentFilter(PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED),Context.RECEIVER_NOT_EXPORTED);else registerReceiver(idleReceiver,new IntentFilter(PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED));
        // A fresh bound instance always prepares a suspended host; it cannot inherit sockets from an older destroyed owner.
        CONTROL.execute(()->{try{NativeBridge.initialize(getApplicationContext());NativeBridge.call("host.stop");main.post(()->{if(destroyed)return;ready=true;refreshHistory();reconcile(false);changed();});}catch(Exception e){error(e);}});
    }
    // A system app-language change must also update Service-owned notification text and channel names.
    @Override public void onConfigurationChanged(android.content.res.Configuration configuration){super.onConfigurationChanged(configuration);getSystemService(NotificationManager.class).createNotificationChannel(new NotificationChannel(CHANNEL,getString(R.string.connection_channel),NotificationManager.IMPORTANCE_LOW));getSystemService(NotificationManager.class).createNotificationChannel(new NotificationChannel("received",getString(R.string.received_channel),NotificationManager.IMPORTANCE_DEFAULT));receiptAlerts.channels();notificationText="";updateNotification();}
    @Override public IBinder onBind(Intent intent){return binder;}
    void attach(String token){clients.add(token);uiVisible=true;startBlocked=false;main.removeCallbacks(pollTick);if(running)poll();}
    void detach(String token){clients.remove(token);uiVisible=!clients.isEmpty();}
    boolean permitted(){return Build.VERSION.SDK_INT<37||checkSelfPermission("android.permission.ACCESS_LOCAL_NETWORK")==android.content.pm.PackageManager.PERMISSION_GRANTED;}
    private boolean trusted(LanDiscovery.Lan lan){return lan!=null&&preferences.getStringSet("trustedScopes",Collections.emptySet()).contains(lan.scope);}
    private boolean eligible(LanDiscovery.Lan lan){return ready&&preferences.getBoolean("receiveEnabled",true)&&trusted(lan)&&permitted();}
    private String name(){return preferences.getString("deviceName",Build.MODEL);}
    void fromUi(){reconcile(!clients.isEmpty());}
    @Override public int onStartCommand(Intent intent,int flags,int startId){
        if(intent!=null&&STOP.equals(intent.getAction())){preferences.edit().putBoolean("receiveEnabled",false).apply();reconcile(false);endForeground();return START_NOT_STICKY;}
        if(intent==null||!START.equals(intent.getAction())){stopSelf();return START_NOT_STICKY;}
        // Promote immediately after an explicitly visible-UI start; eligibility is checked again before opening sockets.
        startRequested=false;try{startForeground(STATUS_ID,notification(getString(R.string.preparing_service)),ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE);foreground=true;reconcile(false);}catch(RuntimeException e){startBlocked=true;error(e);endForeground();}
        return START_NOT_STICKY;
    }
    void reconcile(boolean userVisible){
        if(destroyed||!ready)return;LanDiscovery.Lan lan=LanDiscovery.current(this);boolean allowed=eligible(lan);if(allowed&&foreground&&offlineDeadline!=0&&SystemClock.elapsedRealtime()>=offlineDeadline&&!userVisible){endForeground();return;}if(!allowed&&userVisible)startBlocked=false;
        if(allowed&&!foreground){if(userVisible&&!startRequested&&!startBlocked){startRequested=true;try{startForegroundService(new Intent(this,TransferService.class).setAction(START));}catch(RuntimeException e){startRequested=false;startBlocked=true;error(e);}}return;}
        String next=allowed&&foreground?lan.identity()+"|"+name():"";
        if(next.equals(desired)&&(!allowed||running||starting)){if(!allowed)inactive(lan);else updateNotification();return;}
        if(allowed){offlineDeadline=0;main.removeCallbacks(expireReconnect);}desired=next;final long version=++generation;running=false;starting=allowed&&foreground;main.removeCallbacks(pollTick);peers=new JSONArray();registrations=new JSONArray();discovery.stop();releaseWake();
        CONTROL.execute(()->{try{NativeBridge.call("host.stop");connectivity.bindProcessToNetwork(null);if(!allowed)refreshHistory();if(!allowed||version!=generation||destroyed)return;
            if(!connectivity.bindProcessToNetwork(lan.network))throw new IOException(getString(R.string.lan_unavailable));
            JSONObject params=NativeBridge.paths().put("display_name",name()).put("listen_addr",lan.address+":0").put("allowed_networks",new JSONArray().put(lan.cidr));JSONObject advert=(JSONObject)NativeBridge.call("host.start",params);
            main.post(()->{if(version!=generation||destroyed||!foreground)return;starting=false;running=true;restartDeadline=0;main.removeCallbacks(expireReconnect);discovery.start(lan);discovery.advertise(advert,name());updateNotification();changed();poll();});
        }catch(Exception e){main.post(()->{if(version==generation){starting=false;desired="";
                // The shared host refuses recovery while an interrupted socket is draining; retry this existing FGS only within a deadline.
                if(e.getMessage()!=null&&e.getMessage().startsWith("BUSY")&&foreground){if(restartDeadline==0)restartDeadline=SystemClock.elapsedRealtime()+10000;if(SystemClock.elapsedRealtime()<restartDeadline){main.postDelayed(()->reconcile(false),500);return;}}
                startBlocked=true;error(e);endForeground();}});}});
        if(!allowed)inactive(lan);changed();
    }
    private void inactive(LanDiscovery.Lan lan){if(!foreground)return;updateNotification();main.removeCallbacks(expireReconnect);
        // Keep only a bounded existing-FGS reconnect window; never relaunch an expired Service from the background.
        if(lan==null&&preferences.getBoolean("receiveEnabled",true)&&permitted()){if(offlineDeadline==0)offlineDeadline=SystemClock.elapsedRealtime()+RECONNECT_GRACE_MS;main.postDelayed(expireReconnect,Math.max(0,offlineDeadline-SystemClock.elapsedRealtime()));}else endForeground();
    }
    // History remains a Service snapshot even with receiving disabled; Activity polling must not overwrite a DB read with stale empty state.
    void refreshHistory(){if(!ready||destroyed)return;WORK.execute(()->{try{JSONArray records=(JSONArray)NativeBridge.call("inbox.list",new JSONObject().put("limit",100));main.post(()->{if(!destroyed){history=records;changed();}});}catch(Exception e){error(e);}});}
    // Idle background presence is sampled less often than active transfers/UI to avoid desktop-rate TLS probes all day.
    private void poll(){main.removeCallbacks(pollTick);if(!running||destroyed||(polling&&pollingGeneration==generation))return;polling=true;long version=generation;pollingGeneration=version;String transfer=activeId;
        WORK.execute(()->{try{JSONArray devices=(JSONArray)NativeBridge.call("devices.list",new JSONObject().put("available_only",true));JSONArray records=(JSONArray)NativeBridge.call("inbox.list",new JSONObject().put("limit",100));JSONArray requests=(JSONArray)NativeBridge.call("registration.requests");JSONObject advert=(JSONObject)NativeBridge.call("host.advertisement");JSONObject progress=null;if(transfer!=null)try{progress=(JSONObject)NativeBridge.call("transfers.get",new JSONObject().put("transfer_id",transfer));}catch(Exception ignored){}deliverNotifications(devices);JSONObject measured=progress;
            main.post(()->{if(pollingGeneration==version)polling=false;if(version!=generation||!running||destroyed)return;peers=devices;history=records;registrations=requests;if(transfer!=null&&transfer.equals(activeId)&&measured!=null)fraction=MainActivity.fileFraction(measured);completed.entrySet().removeIf(e->e.getValue().until<=SystemClock.elapsedRealtime());discovery.advertise(advert,name());registrationNotification();maintainActiveWake(records,measured);updateNotification();main.postDelayed(pollTick,clients.isEmpty()&&!sending&&!transferActive?IDLE_POLL_MS:POLL_MS);});
        }catch(Exception e){main.post(()->{if(pollingGeneration==version)polling=false;if(version==generation&&running){error(e);main.postDelayed(pollTick,IDLE_POLL_MS);}});}});
    }
    boolean send(String peer,String body,ArrayList<File> paths,boolean files){if(!running||sending)return false;String id=UUID.randomUUID().toString();activeId=id;activePeer=peer;sending=true;fraction=Double.NaN;completed.remove(peer);activeWake.acquire(ACTIVE_WAKE_MS);updateNotification();
        // A send worker captures only immutable attempt data and this Service; UI destruction cannot cancel or duplicate it.
        WORK.execute(()->{String state="failed",failure=null;try{JSONObject params=new JSONObject().put("transfer_id",id).put("to",peer).put("kind",files?"files":MainActivity.messageKind(body));if(files){JSONArray array=new JSONArray();for(File f:paths)array.put(f.getCanonicalPath());params.put("paths",array);}else params.put("body",body);JSONObject result=(JSONObject)NativeBridge.call("transfers.send_discovered",params);state=result.optString("state");if(!"completed".equals(state))failure=NativeBridge.transferError(result);}catch(Exception e){failure=e.getMessage();}String outcome=state,error=failure;
            main.post(()->{if(!id.equals(activeId)||destroyed)return;lastOutcome=id;lastOutcomeState=outcome;lastOutcomeFiles=files;lastOutcomeBody=body;if("completed".equals(outcome)){completed.put(peer,new Completion(id));if(files)clearSentDraft(paths);else if(preferences.getString("messageDraft","").equals(body))preferences.edit().putString("messageDraft","").apply();}else lastError=error==null?getString(R.string.check_result):error;notifySendResult(id,outcome);activeId=null;activePeer=null;sending=false;fraction=Double.NaN;maintainActiveWake(history,null);refreshHistory();updateNotification();changed();});
        });return true;
    }
    private void clearSentDraft(ArrayList<File> sent){try{JSONArray current=new JSONArray(preferences.getString("attachments","[]"));Set<String> remove=new HashSet<>(),keep=new HashSet<>();for(File file:sent)remove.add(file.getCanonicalPath());JSONArray remaining=new JSONArray();for(int i=0;i<current.length();i++)if(!remove.contains(current.getString(i))){remaining.put(current.getString(i));keep.add(current.getString(i));}preferences.edit().putString("attachments",remaining.toString()).apply();
        // Delete only completed private copies, never picker originals or a newer draft; unknown/failed attempts remain retryable.
        String root=NativeBridge.outgoing.getCanonicalPath()+File.separator;for(File file:sent){String path=file.getCanonicalPath();if(!keep.contains(path)&&path.equals(file.getAbsolutePath())&&path.startsWith(root)){file.delete();File folder=file.getParentFile();while(folder!=null&&folder.getCanonicalPath().startsWith(root)&&folder.delete())folder=folder.getParentFile();}}
    }catch(Exception e){error(e);}}
    private void changed(){sendBroadcast(new Intent(STATE_CHANGED).setPackage(getPackageName()),STATE_PERMISSION);}
    private void notifySendResult(String id,String state){NotificationManager manager=getSystemService(NotificationManager.class);if(!manager.areNotificationsEnabled())return;manager.createNotificationChannel(new NotificationChannel("received",getString(R.string.received_channel),NotificationManager.IMPORTANCE_DEFAULT));String result="completed".equals(state)?getString(R.string.transfer_completed):"partial".equals(state)?getString(R.string.partial_transfer):"cancelled".equals(state)?getString(R.string.transfer_cancelled):getString(R.string.transfer_check_result);Intent intent=new Intent(this,MainActivity.class).setData(Uri.parse("fta://receipt/"+id)).putExtra("transfer_id",id).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);PendingIntent receipt=PendingIntent.getActivity(this,id.hashCode(),intent,PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);manager.notify(id,0,new Notification.Builder(this,"received").setSmallIcon(android.R.drawable.stat_sys_upload_done).setContentTitle("FTA "+result).setContentText(getString(R.string.check_history)).setContentIntent(receipt).setVisibility(Notification.VISIBILITY_PRIVATE).setAutoCancel(true).build());}
    void cancel(){String id=activeId;if(id!=null)CONTROL.execute(()->{try{NativeBridge.call("transfers.cancel",new JSONObject().put("transfer_id",id));}catch(Exception e){error(e);}});}
    private void maintainActiveWake(JSONArray records,JSONObject measured){boolean active=sending;long bytes=0;for(int i=0;i<records.length();i++){JSONObject r=records.optJSONObject(i);if(r==null)continue;String state=r.optString("state");if(Arrays.asList("sending","receiving","verifying","committing").contains(state)){active=true;JSONArray items=r.optJSONArray("items");if(items!=null)for(int j=0;j<items.length();j++)bytes+=items.optJSONObject(j).optLong("bytes_transferred");}}if(measured!=null)bytes+=measured.optLong("bytes_transferred");transferActive=active;if(active){if(bytes!=wakeBytes){activeWake.acquire(ACTIVE_WAKE_MS);wakeBytes=bytes;}}else releaseWake();}
    private void releaseWake(){if(activeWake.isHeld())activeWake.release();wakeBytes=-1;}
    private PendingIntent open(){return PendingIntent.getActivity(this,STATUS_ID,new Intent(this,MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),PendingIntent.FLAG_IMMUTABLE|PendingIntent.FLAG_UPDATE_CURRENT);}
    private Notification notification(String text){PendingIntent stop=PendingIntent.getService(this,STATUS_ID,new Intent(this,TransferService.class).setAction(STOP),PendingIntent.FLAG_IMMUTABLE|PendingIntent.FLAG_UPDATE_CURRENT);return new Notification.Builder(this,CHANNEL).setSmallIcon(android.R.drawable.stat_sys_upload).setContentTitle(getString(R.string.transfer_notification)).setContentText(text).setContentIntent(open()).setOngoing(true).setOnlyAlertOnce(true).setVisibility(Notification.VISIBILITY_PRIVATE).addAction(new Notification.Action.Builder(null,getString(R.string.open),open()).build()).addAction(new Notification.Action.Builder(null,getString(R.string.stop),stop).build()).build();}
    private void updateNotification(){if(!foreground)return;String text=running?(getSystemService(PowerManager.class).isDeviceIdleMode()?getString(R.string.idle_network):sending?getString(R.string.sending_device):getString(R.string.background_ready)):getString(R.string.waiting_lan);boolean permission=getSystemService(NotificationManager.class).areNotificationsEnabled();if(text.equals(notificationText)&&permission==lastNotificationPermission)return;notificationText=text;lastNotificationPermission=permission;getSystemService(NotificationManager.class).notify(STATUS_ID,notification(text));}
    private void registrationNotification(){String id=registrations.length()>0?registrations.optJSONObject(0).optString("session_id"):"";if(id.equals(registrationNotice))return;registrationNotice=id;NotificationManager manager=getSystemService(NotificationManager.class);if(id.isEmpty())manager.cancel(REGISTRATION_ID);else manager.notify(REGISTRATION_ID,new Notification.Builder(this,CHANNEL).setSmallIcon(android.R.drawable.stat_sys_upload).setContentTitle(getString(R.string.registration_notification)).setContentText(getString(R.string.open_registration)).setContentIntent(open()).setAutoCancel(true).setVisibility(Notification.VISIBILITY_PRIVATE).build());}
    // UI awareness and unread tracking are independent of Android's notification permission.
    private void deliverNotifications(JSONArray devices)throws Exception{
        String consumer="android-ui";JSONObject cursor=(JSONObject)NativeBridge.call("events.cursor",new JSONObject().put("consumer",consumer));
        JSONObject page=(JSONObject)NativeBridge.call("events.list",new JSONObject().put("after",cursor.optLong("sequence")).put("limit",EVENT_PAGE_SIZE));JSONArray events=page.optJSONArray("events");if(events==null)return;
        for(int i=0;i<events.length();i++){
            JSONObject event=events.getJSONObject(i);ReceiptAlerts.Notice notice=receiptAlerts.deliver(event,uiVisible,devices);
            if(notice!=null)main.post(()->{if(!destroyed){if(uiVisible)receiptNotice=notice;changed();}});
            NativeBridge.call("events.ack",new JSONObject().put("consumer",consumer).put("sequence",event.optLong("sequence")));
        }
    }
    String status(){if(!ready)return getString(R.string.preparing_engine);LanDiscovery.Lan lan=LanDiscovery.current(this);if(!preferences.getBoolean("receiveEnabled",true))return getString(R.string.sending_and_receiving_paused);if(lan==null)return getString(R.string.no_lan);if(!trusted(lan))return getString(R.string.confirm_network);if(!permitted())return getString(R.string.lan_permission_required);if(running)return getSystemService(NotificationManager.class).areNotificationsEnabled()?getString(R.string.background_ready):getString(R.string.receiving_notifications_denied);return !startBlocked?getString(R.string.preparing_service):getString(R.string.service_retry);}
    private void error(Exception e){main.post(()->lastError=e.getMessage()==null?getString(R.string.service_failed):e.getMessage());}
    private void endForeground(){foreground=false;startRequested=false;offlineDeadline=0;main.removeCallbacks(expireReconnect);stopForeground(STOP_FOREGROUND_REMOVE);getSystemService(NotificationManager.class).cancel(REGISTRATION_ID);stopSelf();}
    @Override public void onDestroy(){destroyed=true;generation++;running=false;foreground=false;discovery.stop();releaseWake();main.removeCallbacksAndMessages(null);try{unregisterReceiver(idleReceiver);}catch(Exception ignored){}try{connectivity.unregisterNetworkCallback(networkCallback);}catch(Exception ignored){}CONTROL.execute(()->{if(owner!=null&&owner!=this)return;try{NativeBridge.call("host.stop");connectivity.bindProcessToNetwork(null);}catch(Exception ignored){}});if(owner==this)owner=null;history=new JSONArray();peers=new JSONArray();registrations=new JSONArray();completed.clear();lastOutcomeBody=null;super.onDestroy();}
}
