package app.fta.android;

import android.app.*;
import android.content.*;
import android.media.AudioAttributes;
import android.net.Uri;
import android.os.Build;
import android.provider.Settings;
import org.json.*;
import java.io.IOException;
import java.text.NumberFormat;
import java.util.Arrays;

// Receipt bookkeeping survives process death independently of the engine's delivery cursor.
// Store only locators/sequences, never message bodies or file contents in preferences.
final class ReceiptAlerts {
    private static final Object LOCK=new Object();
    static final String LEGACY_CHANNEL="received",ALERT_CHANNEL="received-alerts-v1";
    private final Context context;
    private final SharedPreferences preferences;
    static final class Notice {
        final String id,title,peer,state;final long sequence;
        Notice(String id,long sequence,String title,String peer,String state){this.id=id;this.sequence=sequence;this.title=title;this.peer=peer;this.state=state;}
    }
    ReceiptAlerts(Context context){this.context=context.getApplicationContext();preferences=context.getSharedPreferences("receipt-alerts",Context.MODE_PRIVATE);channels();}
    // Existing user changes (including a muted/blocked channel) take precedence over the new default.
    static boolean preserveChannel(NotificationChannel old){return old!=null&&(old.hasUserSetImportance()||old.getImportance()==NotificationManager.IMPORTANCE_NONE||old.getSound()==null||(Build.VERSION.SDK_INT>=30&&old.hasUserSetSound()));}
    String backgroundChannel(){return preserveChannel(context.getSystemService(NotificationManager.class).getNotificationChannel(LEGACY_CHANNEL))?LEGACY_CHANNEL:ALERT_CHANNEL;}
    void channels(){
        NotificationManager manager=context.getSystemService(NotificationManager.class);
        manager.createNotificationChannel(new NotificationChannel(LEGACY_CHANNEL,context.getString(R.string.received_channel),NotificationManager.IMPORTANCE_DEFAULT));
        if(backgroundChannel().equals(ALERT_CHANNEL)){
            NotificationChannel channel=new NotificationChannel(ALERT_CHANNEL,context.getString(R.string.receive_alerts),NotificationManager.IMPORTANCE_HIGH);
            channel.enableVibration(true);channel.setSound(Settings.System.DEFAULT_NOTIFICATION_URI,new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION).build());manager.createNotificationChannel(channel);
        }
    }
    private JSONObject data()throws JSONException{return new JSONObject(preferences.getString("state","{\"sequence\":0,\"unread\":{}}"));}
    private void save(JSONObject value)throws IOException{if(!preferences.edit().putString("state",value.toString()).commit())throw new IOException(context.getString(R.string.operation_failed));}
    int count(){synchronized(LOCK){try{return data().getJSONObject("unread").length();}catch(JSONException e){return 0;}}}
    void read(String id)throws Exception{synchronized(LOCK){JSONObject value=data();value.getJSONObject("unread").remove(id);save(value);}context.getSystemService(NotificationManager.class).cancel(id,0);}
    Notice deliver(JSONObject event,boolean uiVisible,JSONArray peers)throws Exception{synchronized(LOCK){
        long sequence=event.getLong("sequence");JSONObject value=data(),record=event.optJSONObject("record");
        if(sequence<=value.optLong("sequence")||record==null||!"receive".equals(record.optString("direction"))||!Arrays.asList("completed","partial","failed","cancelled","unknown").contains(record.optString("state")))return null;
        String id=event.getString("transfer_id"),state=record.optString("state"),kind=record.optString("kind"),peer="FTA";
        for(int i=0;i<peers.length();i++){JSONObject p=peers.optJSONObject(i);if(p!=null&&p.optString("device_id").equals(record.optString("peer_id"))){peer=p.optString("display_name","FTA");break;}}
        int count=0;JSONArray items=record.optJSONArray("items");if(items!=null)for(int i=0;i<items.length();i++)if("completed".equals(items.getJSONObject(i).optString("state")))count++;
        String title=("files".equals(kind)?context.getString(R.string.files):"url".equals(kind)?"URL":context.getString(R.string.text))+(count>0?" · "+NumberFormat.getIntegerInstance().format(count):"")+" · "+context.getString("completed".equals(state)?R.string.completed:"partial".equals(state)?R.string.partially_completed:"failed".equals(state)?R.string.failed:"cancelled".equals(state)?R.string.transfer_cancelled:R.string.check_result);
        value.getJSONObject("unread").put(id,sequence);save(value);
        NotificationManager manager=context.getSystemService(NotificationManager.class);
        if(manager.areNotificationsEnabled()){
            Intent intent=new Intent(context,MainActivity.class).setData(Uri.parse("fta://receipt/"+id)).putExtra("transfer_id",id).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
            PendingIntent tap=PendingIntent.getActivity(context,id.hashCode(),intent,PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);
            // A stable tag replaces a submission retry. Visible UI owns the banner; background uses heads-up importance.
            manager.notify(id,0,new Notification.Builder(context,uiVisible?LEGACY_CHANNEL:backgroundChannel()).setSmallIcon(android.R.drawable.stat_sys_download_done).setContentTitle(title).setContentText(peer).setContentIntent(tap).setVisibility(Notification.VISIBILITY_PRIVATE).setAutoCancel(true).setOnlyAlertOnce(true).build());
        }
        value.put("sequence",sequence);save(value);
        return new Notice(id,sequence,title,peer,state);
    }}
}
