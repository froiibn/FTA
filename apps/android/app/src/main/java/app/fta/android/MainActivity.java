package app.fta.android;

import android.Manifest;
import android.animation.ValueAnimator;
import android.app.*;
import android.content.*;
import android.content.pm.PackageManager;
import android.graphics.*;
import android.graphics.drawable.*;
import android.net.*;
import android.os.*;
import android.provider.*;
import android.text.*;
import android.view.*;
import android.view.inputmethod.InputMethodManager;
import android.widget.*;
import org.json.*;
import java.io.*;
import java.net.URI;
import java.util.*;
import java.util.concurrent.*;

// Framework controls and one Rust engine preserve native interaction without adding an Android UI dependency.
public final class MainActivity extends Activity {
    static final int WHITE=Color.WHITE;
    private static final int PICK_PHOTOS=101,PICK_FILES=102,CAPTURE=103,LAN_PERMISSION=104,NOTIFICATIONS=105,OPEN_RECEIVED=106;
    private static final int MAX_ATTACHMENTS=100, POLL_MS=750, EVENT_PAGE_SIZE=1;
    private static final long COMPLETION_MS=1500, BLINK_CYCLE_MS=600;
    // Lifecycle controls are serialized globally, so recreation cannot stop a newer listener from an old worker.
    private static final ExecutorService CONTROL=Executors.newSingleThreadExecutor();
    private static final ExecutorService WORK=Executors.newFixedThreadPool(3);
    private final Handler main=new Handler(Looper.getMainLooper());
    private SharedPreferences preferences;
    private LanDiscovery discovery;
    private ConnectivityManager connectivity;
    private ConnectivityManager.NetworkCallback networkCallback;
    private volatile boolean visible,ready,destroyed;
    private volatile long generation;
    private String desired="",selected="",preferredPeer="",activeId,activePeer,messageDraft="",cameraToken;
    private boolean running,starting,sending,importing,polling;
    private long pollingGeneration=-1;
    private double fraction=Double.NaN;
    private JSONArray peers=new JSONArray(),history=new JSONArray(),registrations=new JSONArray();
    private final ArrayList<File> attachments=new ArrayList<>();
    private final HashMap<String,Receipt> completed=new HashMap<>();
    private final HashSet<String> shownRequests=new HashSet<>();
    private int tab;
    private LinearLayout root,body,deviceRow,attachmentRow,historyRows;
    private TextView status,empty,attachmentText;
    private EditText message;
    private Button photoButton,fileButton;
    private ImageButton sendButton;
    private final RadioButton[] tabButtons=new RadioButton[3];
    private Toolbar toolbar;
    private LinearLayout navigation;
    private MenuItem refreshAction;
    private int background,surface,textPrimary,textSecondary,accent,outline,transferGreen,completionInk;
    private String lastError,pendingReceipt,renderedPeers,renderedHistory,renderedAttachments;
    private List<Uri> deferredUris;
    private Runnable deferredCleanup;
    private static final class Receipt {final String id;final long until;Receipt(String id){this.id=id;until=SystemClock.elapsedRealtime()+COMPLETION_MS;}}

    @Override public void onCreate(Bundle state){super.onCreate(state);
        // Theme attributes keep all Android controls and custom status drawing consistent with system light/night mode.
        background=themeColor(android.R.attr.colorBackground);surface=themeColor(android.R.attr.colorBackgroundFloating);textPrimary=themeColor(android.R.attr.textColorPrimary);textSecondary=themeColor(android.R.attr.textColorSecondary);accent=themeColor(android.R.attr.colorAccent);outline=withAlpha(textSecondary,0.24f);
        // Green remains semantic progress/success; theme-specific contrast keeps the ring and check legible in both modes.
        boolean night=(getResources().getConfiguration().uiMode&android.content.res.Configuration.UI_MODE_NIGHT_MASK)==android.content.res.Configuration.UI_MODE_NIGHT_YES;transferGreen=night?0xFF81C995:0xFF188038;completionInk=night?Color.BLACK:WHITE;
        preferences=getSharedPreferences("fta",MODE_PRIVATE);messageDraft=preferences.getString("messageDraft","");cameraToken=preferences.getString("cameraToken",null);
        connectivity=getSystemService(ConnectivityManager.class);discovery=new LanDiscovery(this,CONTROL);discovery.failure=text->fail(new IOException(text));
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        createRoot();showTab(state==null?0:state.getInt("selectedTab",0));
        networkCallback=new ConnectivityManager.NetworkCallback(){
            @Override public void onAvailable(Network n){main.post(()->updateLifecycle());}
            @Override public void onLost(Network n){main.post(()->updateLifecycle());}
            @Override public void onCapabilitiesChanged(Network n,NetworkCapabilities c){main.post(()->updateLifecycle());}
            @Override public void onLinkPropertiesChanged(Network n,LinkProperties l){main.post(()->updateLifecycle());}
        };
        connectivity.registerNetworkCallback(new NetworkRequest.Builder().addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN).build(),networkCallback);
        CONTROL.execute(()->{try{NativeBridge.initialize(getApplicationContext());main.post(()->{if(destroyed)return;ready=true;restoreAttachments();pruneStaging();updateLifecycle();render();if(pendingReceipt!=null){String receipt=pendingReceipt;pendingReceipt=null;openReceipt(receipt);}if(deferredUris!=null){List<Uri> uris=deferredUris;Runnable cleanup=deferredCleanup;deferredUris=null;deferredCleanup=null;importUris(uris,cleanup);}});}catch(Exception e){fail(e);}});
        // Cold-start notification navigation waits for actual engine readiness rather than an arbitrary delay.
        pendingReceipt=getIntent().getStringExtra("transfer_id");
    }
    @Override protected void onStart(){super.onStart();visible=true;updateLifecycle();}
    @Override protected void onStop(){visible=false;persistDraft();updateLifecycle();super.onStop();}
    @Override protected void onDestroy(){destroyed=true;visible=false;generation++;discovery.stop();main.removeCallbacksAndMessages(null);try{connectivity.unregisterNetworkCallback(networkCallback);}catch(Exception ignored){}super.onDestroy();}
    @Override protected void onNewIntent(Intent intent){super.onNewIntent(intent);setIntent(intent);String id=intent.getStringExtra("transfer_id");if(id!=null)openReceipt(id);}
    @Override public void onRequestPermissionsResult(int request,String[] permissions,int[] grants){super.onRequestPermissionsResult(request,permissions,grants);if(request==LAN_PERMISSION){updateLifecycle();showTab(tab);} }

    private boolean permitted(){return Build.VERSION.SDK_INT<37||checkSelfPermission("android.permission.ACCESS_LOCAL_NETWORK")==PackageManager.PERMISSION_GRANTED;}
    private boolean trusted(LanDiscovery.Lan lan){return lan!=null&&preferences.getStringSet("trustedScopes",Collections.emptySet()).contains(lan.scope);}
    private String name(){return preferences.getString("deviceName",Build.MODEL);}
    // Visibility and current LAN govern sockets; saved consent survives suspension and DHCP renewals.
    private void updateLifecycle(){
        if(!ready||destroyed)return;
        LanDiscovery.Lan lan=LanDiscovery.current(this);
        boolean allowed=visible&&preferences.getBoolean("receiveEnabled",true)&&trusted(lan)&&permitted();
        String next=allowed?lan.identity():"";
        if(next.equals(desired)&&(!allowed||running||starting)){renderStatus();return;}
        desired=next;final long version=++generation;
        running=false;starting=allowed;peers=new JSONArray();selected="";discovery.stop();render();
        CONTROL.execute(()->{try{
            NativeBridge.call("host.stop");connectivity.bindProcessToNetwork(null);
            if(!allowed||version!=generation)return;
            if(!connectivity.bindProcessToNetwork(lan.network))throw new IOException("선택한 LAN을 사용할 수 없습니다");
            JSONObject params=NativeBridge.paths().put("display_name",name()).put("listen_addr",lan.address+":0").put("allowed_networks",new JSONArray().put(lan.cidr));
            JSONObject advert=(JSONObject)NativeBridge.call("host.start",params);
            main.post(()->{if(version!=generation||!visible||destroyed)return;starting=false;running=true;discovery.start(lan);discovery.advertise(advert,name());render();poll();});
        }catch(Exception e){main.post(()->{if(version==generation){starting=false;desired="";fail(e);}});}});
        if(!allowed)loadHistory();
    }
    // Generation/transfer IDs reject delayed results after a path change or a newer send.
    private void poll(){
        if(!visible||!running||destroyed||(polling&&pollingGeneration==generation))return;
        polling=true;final long version=generation;pollingGeneration=version;final String transfer=activeId;
        WORK.execute(()->{try{
            JSONArray devices=(JSONArray)NativeBridge.call("devices.list",new JSONObject().put("available_only",true));
            JSONArray records=(JSONArray)NativeBridge.call("inbox.list",new JSONObject().put("limit",100));
            JSONArray requests=(JSONArray)NativeBridge.call("registration.requests");
            JSONObject advert=(JSONObject)NativeBridge.call("host.advertisement");
            JSONObject progress=null;
            if(transfer!=null)try{progress=(JSONObject)NativeBridge.call("transfers.get",new JSONObject().put("transfer_id",transfer));}catch(Exception ignored){}
            deliverNotifications();final JSONObject measured=progress;
            main.post(()->{if(pollingGeneration==version)polling=false;if(version!=generation||!running||destroyed)return;peers=devices;history=records;registrations=requests;
                if(transfer!=null&&transfer.equals(activeId)&&measured!=null)fraction=fileFraction(measured);
                discovery.advertise(advert,name());render();presentRegistration();main.postDelayed(this::poll,POLL_MS);});
        }catch(Exception e){main.post(()->{if(pollingGeneration==version)polling=false;if(version==generation&&running){fail(e);main.postDelayed(this::poll,POLL_MS*2);}});}});
    }
    private void loadHistory(){if(!ready)return;WORK.execute(()->{try{JSONArray records=(JSONArray)NativeBridge.call("inbox.list",new JSONObject().put("limit",100));main.post(()->{if(destroyed)return;history=records;if(tab==1)renderHistory();});}catch(Exception ignored){}});}
    static double fileFraction(JSONObject transfer){if(!"files".equals(transfer.optString("kind")))return Double.NaN;JSONArray items=transfer.optJSONArray("items");double total=0,sent=0;if(items!=null)for(int i=0;i<items.length();i++){JSONObject item=items.optJSONObject(i);if(item!=null){total+=item.optDouble("size",0);sent+=item.optDouble("bytes_transferred",0);}}return total>0?Math.min(1,Math.max(0,sent/total)):Double.NaN;}
    private void presentRegistration(){if(!visible||isFinishing())return;for(int i=0;i<registrations.length();i++){JSONObject request=registrations.optJSONObject(i);if(request==null)continue;String id=request.optString("session_id");if(!shownRequests.add(id))continue;
        new AlertDialog.Builder(this).setTitle("기기 등록 요청").setMessage(request.optString("display_name")+"\n"+request.optString("os")+" · "+request.optString("kind")+" · "+request.optInt("item_count")+"개\n등록하면 이후 온라인 상태에서 바로 받을 수 있습니다.")
            .setPositiveButton("등록",(d,w)->registration(id,true)).setNegativeButton("거절",(d,w)->registration(id,false)).setOnCancelListener(d->registration(id,false)).show();break;}}
    private void registration(String id,boolean accept){CONTROL.execute(()->{try{NativeBridge.call(accept?"registration.accept":"registration.reject",new JSONObject().put("session_id",id));}catch(Exception e){fail(e);}});}

    // Toolbar and checked native navigation replace the iOS-like brand header and text-only tab buttons.
    private void createRoot(){
        root=new LinearLayout(this);root.setOrientation(LinearLayout.VERTICAL);root.setBackgroundColor(background);
        root.setOnApplyWindowInsetsListener((v,insets)->{
            if(Build.VERSION.SDK_INT>=30){android.graphics.Insets bars=insets.getInsets(WindowInsets.Type.systemBars());int keyboard=insets.getInsets(WindowInsets.Type.ime()).bottom;v.setPadding(bars.left,bars.top,bars.right,Math.max(bars.bottom,keyboard));if(navigation!=null)navigation.setVisibility(keyboard>0?View.GONE:View.VISIBLE);}
            else v.setPadding(insets.getSystemWindowInsetLeft(),insets.getSystemWindowInsetTop(),insets.getSystemWindowInsetRight(),insets.getSystemWindowInsetBottom());return insets;
        });
        toolbar=new Toolbar(this);toolbar.setBackgroundColor(surface);toolbar.setTitleTextColor(textPrimary);toolbar.setSubtitleTextColor(textSecondary);toolbar.setContentInsetsRelative(dp(16),dp(8));
        // Toolbar collapses the start inset against its title margin; include both to keep a 12dp logo gap.
        toolbar.setTitleMarginStart(dp(16)+dp(12));
        Bitmap mark=BitmapFactory.decodeResource(getResources(),R.drawable.app_mark);toolbar.setLogo(new BitmapDrawable(getResources(),Bitmap.createScaledBitmap(mark,dp(32),dp(32),true)));
        refreshAction=toolbar.getMenu().add("기기 새로고침");Drawable refresh=getDrawable(R.drawable.ic_refresh);refresh.setTint(textPrimary);refreshAction.setIcon(refresh);refreshAction.setShowAsAction(MenuItem.SHOW_AS_ACTION_ALWAYS);
        toolbar.setOnMenuItemClickListener(item->{if(tab==1)loadHistory();else if(running)poll();else updateLifecycle();return true;});
        root.addView(toolbar,new LinearLayout.LayoutParams(-1,dp(56)));root.addView(divider());
        body=new LinearLayout(this);body.setOrientation(LinearLayout.VERTICAL);root.addView(body,new LinearLayout.LayoutParams(-1,0,1));
        navigation=new LinearLayout(this);navigation.setOrientation(LinearLayout.VERTICAL);navigation.setBackgroundColor(surface);navigation.addView(divider());
        RadioGroup tabs=new RadioGroup(this);tabs.setOrientation(LinearLayout.HORIZONTAL);tabs.setGravity(Gravity.CENTER_VERTICAL);
        String[] titles={"보내기","기록","설정"};int[] icons={R.drawable.ic_send,R.drawable.ic_history,R.drawable.ic_settings};
        android.content.res.ColorStateList colors=new android.content.res.ColorStateList(new int[][]{new int[]{android.R.attr.state_checked},new int[]{}},new int[]{accent,textSecondary});
        for(int i=0;i<titles.length;i++){final int index=i;RadioButton button=new RadioButton(this);button.setId(View.generateViewId());button.setButtonDrawable(null);button.setText(titles[i]);button.setTextSize(12);button.setGravity(Gravity.CENTER);button.setPadding(0,dp(8),0,dp(8));button.setTextColor(colors);button.setCompoundDrawablesWithIntrinsicBounds(null,getDrawable(icons[i]),null,null);button.setCompoundDrawableTintList(colors);button.setCompoundDrawablePadding(dp(4));button.setBackground(selectable(false));button.setOnClickListener(v->showTab(index));tabButtons[i]=button;tabs.addView(button,new RadioGroup.LayoutParams(0,dp(64),1));}
        navigation.addView(tabs);root.addView(navigation);setContentView(root);
    }
    @Override protected void onSaveInstanceState(Bundle state){state.putInt("selectedTab",tab);super.onSaveInstanceState(state);}
    private void showTab(int value){
        if(message!=null)messageDraft=message.getText().toString();tab=value;
        for(int i=0;i<tabButtons.length;i++)if(tabButtons[i]!=null){tabButtons[i].setChecked(i==value);tabButtons[i].setTypeface(null,i==value?Typeface.BOLD:Typeface.NORMAL);}
        toolbar.setTitle(value==0?"FTA":value==1?"전송 기록":"설정");toolbar.setSubtitle(value==0?"File Transfer Anything":null);refreshAction.setVisible(value!=2);refreshAction.setTitle(value==1?"기록 새로고침":"기기 새로고침");
        body.removeAllViews();message=null;status=null;deviceRow=null;historyRows=null;
        if(tab==0)buildSend();else if(tab==1)buildHistory();else buildSettings();render();
    }
    private LinearLayout column(){LinearLayout c=new LinearLayout(this);c.setOrientation(LinearLayout.VERTICAL);c.setPadding(dp(16),dp(16),dp(16),dp(16));return c;}
    private ScrollView scroll(LinearLayout content){ScrollView s=new ScrollView(this);s.setFillViewport(true);s.addView(content,new ScrollView.LayoutParams(-1,-2));return s;}
    private TextView section(String title){TextView text=label(title,14);text.setTypeface(null,Typeface.BOLD);text.setTextColor(accent);text.setPadding(0,dp(20),0,dp(8));return text;}
    private void buildSend(){
        renderedPeers=null;renderedAttachments=null;LinearLayout content=column();body.addView(scroll(content),new LinearLayout.LayoutParams(-1,0,1));
        status=label("",14);status.setTextColor(textSecondary);status.setPadding(0,0,0,dp(8));content.addView(status);content.addView(section("연결 가능한 기기"));
        HorizontalScrollView devices=new HorizontalScrollView(this);devices.setHorizontalScrollBarEnabled(false);deviceRow=new LinearLayout(this);deviceRow.setPadding(0,dp(8),0,dp(8));devices.addView(deviceRow);content.addView(devices);
        empty=label("발견된 기기가 없습니다",16);empty.setTextColor(textSecondary);empty.setPadding(0,dp(24),0,dp(32));content.addView(empty);
        content.addView(section("사진 및 파일"));LinearLayout actions=new LinearLayout(this);photoButton=button("사진",this::photoChoice);fileButton=button("파일",()->pick(false));setButtonIcon(photoButton,R.drawable.ic_photo);setButtonIcon(fileButton,R.drawable.ic_file);actions.addView(photoButton,new LinearLayout.LayoutParams(0,dp(56),1));actions.addView(fileButton,new LinearLayout.LayoutParams(0,dp(56),1));content.addView(actions);
        attachmentText=label("",14);attachmentText.setTextColor(textSecondary);attachmentText.setPadding(0,dp(8),0,0);content.addView(attachmentText);attachmentRow=new LinearLayout(this);attachmentRow.setOrientation(LinearLayout.VERTICAL);content.addView(attachmentRow);
        // The native underlined EditText supplies Android focus, selection and four-line scrolling instead of a pill surface.
        LinearLayout composer=new LinearLayout(this);composer.setOrientation(LinearLayout.VERTICAL);composer.setBackgroundColor(surface);composer.addView(divider());
        LinearLayout row=new LinearLayout(this);row.setGravity(Gravity.BOTTOM);row.setPadding(dp(16),dp(8),dp(8),dp(8));
        message=new EditText(this);message.setText(messageDraft);message.setHint("텍스트 또는 URL");message.setTextSize(16);message.setInputType(android.text.InputType.TYPE_CLASS_TEXT|android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE|android.text.InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);message.setMinLines(1);message.setMaxLines(4);message.setVerticalScrollBarEnabled(true);
        message.addTextChangedListener(new TextWatcher(){public void beforeTextChanged(CharSequence s,int st,int count,int after){}public void onTextChanged(CharSequence s,int st,int before,int count){messageDraft=s.toString();renderActions();}public void afterTextChanged(Editable e){}});
        row.addView(message,new LinearLayout.LayoutParams(0,-2,1));sendButton=new ImageButton(this);sendButton.setImageResource(R.drawable.ic_send);sendButton.setImageTintList(android.content.res.ColorStateList.valueOf(accent));sendButton.setPadding(dp(12),dp(12),dp(12),dp(12));sendButton.setContentDescription("메시지 보내기");sendButton.setBackground(selectable(true));sendButton.setOnClickListener(v->send(false));row.addView(sendButton,new LinearLayout.LayoutParams(dp(48),dp(48)));composer.addView(row);body.addView(composer,new LinearLayout.LayoutParams(-1,-2));
    }
    private void buildHistory(){renderedHistory=null;LinearLayout content=column();historyRows=new LinearLayout(this);historyRows.setOrientation(LinearLayout.VERTICAL);content.addView(historyRows);body.addView(scroll(content),new LinearLayout.LayoutParams(-1,-1));loadHistory();}
    private void buildSettings(){
        LinearLayout c=column();status=label("",14);status.setTextColor(textSecondary);c.addView(status);c.addView(section("네트워크"));
        LanDiscovery.Lan lan=LanDiscovery.current(this);TextView network=label(lan==null?"Wi-Fi 또는 유선 LAN에 연결하세요":lan.interfaceName+" · "+lan.cidr,14);network.setTextColor(textSecondary);network.setPadding(0,0,0,dp(8));c.addView(network);
        Switch trust=new Switch(this);trust.setText("신뢰하는 집 네트워크입니다");trust.setTextSize(16);trust.setMinHeight(dp(56));trust.setChecked(trusted(lan));trust.setEnabled(lan!=null);trust.setOnCheckedChangeListener((b,on)->{Set<String> scopes=new HashSet<>(preferences.getStringSet("trustedScopes",Collections.emptySet()));if(lan!=null){if(on)scopes.add(lan.scope);else scopes.remove(lan.scope);}preferences.edit().putStringSet("trustedScopes",scopes).apply();if(on&&!permitted())requestPermissions(new String[]{"android.permission.ACCESS_LOCAL_NETWORK"},LAN_PERMISSION);updateLifecycle();});c.addView(trust);c.addView(divider());
        Switch receive=new Switch(this);receive.setText("송수신 사용");receive.setTextSize(16);receive.setMinHeight(dp(56));receive.setChecked(preferences.getBoolean("receiveEnabled",true));receive.setOnCheckedChangeListener((b,on)->{preferences.edit().putBoolean("receiveEnabled",on).apply();updateLifecycle();});c.addView(receive);
        TextView note=label("화면이 열려 있는 동안 수신합니다. 앱을 벗어나도 신뢰 설정은 유지됩니다.",14);note.setTextColor(textSecondary);note.setPadding(0,0,0,dp(8));c.addView(note);
        c.addView(section("기기"));EditText name=new EditText(this);name.setText(name());name.setHint("기기 이름");name.setSingleLine(true);name.setTextSize(16);name.setContentDescription("기기 이름");c.addView(name);
        Button save=button("기기 이름 저장",()->{String value=name.getText().toString().trim();if(value.isEmpty()||value.getBytes(java.nio.charset.StandardCharsets.UTF_8).length>128){fail(new IOException("기기 이름은 1~128바이트여야 합니다"));return;}preferences.edit().putString("deviceName",value).apply();desired="restart";updateLifecycle();});LinearLayout.LayoutParams saveParams=new LinearLayout.LayoutParams(-2,dp(48));saveParams.gravity=Gravity.END;c.addView(save,saveParams);
        c.addView(settingsRow("등록된 기기 관리","온라인 상태와 관계없이 등록을 해제할 수 있습니다",R.drawable.ic_network,this::manageDevices));
        c.addView(section("파일 및 권한"));c.addView(settingsRow("받은 파일 열기","Android 파일 앱에서 확인",R.drawable.ic_file,()->{try{Intent intent=new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").addCategory(Intent.CATEGORY_OPENABLE).putExtra(DocumentsContract.EXTRA_INITIAL_URI,DocumentsContract.buildRootUri("app.fta.android.documents","received"));startActivityForResult(intent,OPEN_RECEIVED);}catch(Exception e){fail(e);}}));
        c.addView(settingsRow("수신 알림 허용","전송 결과 알림 설정",R.drawable.ic_notifications,()->{if(Build.VERSION.SDK_INT>=33&&checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)!=PackageManager.PERMISSION_GRANTED)requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS},NOTIFICATIONS);else startActivity(new Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE,getPackageName()));}));
        c.addView(settingsRow("로컬 네트워크 권한 설정","Android 앱 권한 관리",R.drawable.ic_network,()->startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS,Uri.parse("package:"+getPackageName())))));
        c.addView(section("앱 정보"));c.addView(settingsRow("개인정보 처리 안내",null,R.drawable.ic_info,()->showLegal("PrivacyPolicy.txt")));c.addView(settingsRow("오픈소스 고지",null,R.drawable.ic_info,()->showLegal("ThirdPartyNotices.txt")));TextView version=label("FTA 0.1.0 · Android 초안",12);version.setTextColor(textSecondary);version.setPadding(0,dp(16),0,0);c.addView(version);body.addView(scroll(c),new LinearLayout.LayoutParams(-1,-1));
    }
    private View settingsRow(String title,String summary,int icon,Runnable action){
        LinearLayout row=new LinearLayout(this);row.setGravity(Gravity.CENTER_VERTICAL);row.setPadding(0,dp(12),0,dp(12));row.setMinimumHeight(dp(64));row.setBackground(selectable(false));row.setOnClickListener(v->action.run());row.setFocusable(true);
        ImageView image=new ImageView(this);image.setImageResource(icon);image.setImageTintList(android.content.res.ColorStateList.valueOf(textSecondary));LinearLayout.LayoutParams ip=new LinearLayout.LayoutParams(dp(24),dp(24));ip.setMargins(0,0,dp(24),0);row.addView(image,ip);
        LinearLayout labels=new LinearLayout(this);labels.setOrientation(LinearLayout.VERTICAL);labels.addView(label(title,16));if(summary!=null){TextView detail=label(summary,14);detail.setTextColor(textSecondary);detail.setPadding(0,dp(4),0,0);labels.addView(detail);}row.addView(labels,new LinearLayout.LayoutParams(0,-2,1));return row;
    }
    private void render(){if(destroyed)return;renderStatus();if(tab==0){renderPeers();renderActions();renderAttachments();}else if(tab==1)renderHistory();}
    private void renderStatus(){if(status==null)return;LanDiscovery.Lan lan=LanDiscovery.current(this);status.setText(!ready?"엔진 준비 중":running?"이 화면이 열려 있는 동안 수신 가능":lan==null?"LAN 연결 없음":!trusted(lan)?"설정에서 집 네트워크를 확인하세요":!permitted()?"로컬 네트워크 권한을 허용하세요":!preferences.getBoolean("receiveEnabled",true)?"송수신 중지됨":"송수신 준비 중");}
    private void renderPeers(){if(deviceRow==null)return;
        // Stable rows preserve native accessibility focus; polling identical engine data must not recreate controls.
        String snapshot=peers.toString()+"|"+selected+"|"+activeId+"|"+fraction+"|"+completed.toString();if(snapshot.equals(renderedPeers))return;renderedPeers=snapshot;
        deviceRow.removeAllViews();boolean exists=false;
        for(int i=0;i<peers.length();i++){JSONObject p=peers.optJSONObject(i);if(p==null||!p.optBoolean("online"))continue;String id=p.optString("device_id");if(selected.isEmpty()&&id.equals(preferredPeer))selected=id;if(id.equals(selected))exists=true;
            LinearLayout item=new LinearLayout(this);item.setOrientation(LinearLayout.VERTICAL);item.setGravity(Gravity.CENTER_HORIZONTAL);item.setPadding(0,0,dp(16),0);IconTile icon=new IconTile(p);item.addView(icon,new LinearLayout.LayoutParams(dp(78),dp(76)));TextView title=label(p.optString("display_name"),12);title.setGravity(Gravity.CENTER);title.setMaxLines(2);LinearLayout.LayoutParams tp=new LinearLayout.LayoutParams(dp(90),-2);tp.topMargin=dp(8);item.addView(title,tp);String feedback="";Receipt receipt=completed.get(id);if(receipt!=null&&SystemClock.elapsedRealtime()<receipt.until)feedback=", 전송 완료";else if(id.equals(activePeer)&&activeId!=null)feedback=Double.isNaN(fraction)?", 전송 준비 중":", 전송 중 "+(int)(fraction*100)+"%";item.setContentDescription(p.optString("display_name")+(p.optBoolean("paired")?", 온라인":", 최초 등록 필요")+feedback);item.setSelected(id.equals(selected));item.setOnClickListener(v->{if(sending)return;selected=id;preferredPeer=id;renderPeers();renderActions();});deviceRow.addView(item);
        }
        if(!exists)selected="";empty.setVisibility(deviceRow.getChildCount()==0?View.VISIBLE:View.GONE);
    }
    private void renderActions(){if(tab!=0||sendButton==null)return;boolean can=running&&!selected.isEmpty()&&!sending&&!importing;sendButton.setEnabled(can&&!messageDraft.trim().isEmpty());sendButton.setAlpha(sendButton.isEnabled()?1:0.4f);photoButton.setEnabled(!sending&&!importing&&ready);fileButton.setEnabled(!sending&&!importing&&ready);}
    private void renderAttachments(){if(attachmentText==null)return;
        String snapshot=attachments.toString()+"|"+importing+"|"+sending+"|"+running+"|"+selected;if(snapshot.equals(renderedAttachments))return;renderedAttachments=snapshot;
        attachmentText.setText(importing?"선택한 항목 준비 중…":attachments.isEmpty()?"":attachments.size()+"개 선택됨");attachmentRow.removeAllViews();if(!attachments.isEmpty()){
        StringBuilder names=new StringBuilder();for(File file:attachments){if(names.length()>0)names.append("\n");names.append(file.getName());}TextView list=label(names.toString(),12);list.setMaxLines(4);attachmentRow.addView(list);
        LinearLayout buttons=new LinearLayout(this);Button send=button(sending?"취소":"선택 항목 전송",()->{if(sending)cancel();else send(true);});send.setEnabled(sending||(running&&!selected.isEmpty()&&!importing));send.setAlpha(send.isEnabled()?1:0.4f);buttons.addView(send,new LinearLayout.LayoutParams(0,-2,1));Button clear=button("선택 해제",()->{clearAttachments();renderAttachments();});clear.setEnabled(!sending&&!importing);clear.setAlpha(clear.isEnabled()?1:0.4f);buttons.addView(clear);attachmentRow.addView(buttons);
    }else if(sending)attachmentRow.addView(button("전송 취소",this::cancel));}

    private void photoChoice(){PopupMenu menu=new PopupMenu(this,photoButton);menu.getMenu().add("갤러리에서 선택").setOnMenuItemClickListener(i->{pick(true);return true;});menu.getMenu().add("촬영").setOnMenuItemClickListener(i->{capture();return true;});menu.show();}
    private void pick(boolean photos){if(!ready)return;if(attachments.size()>=MAX_ATTACHMENTS){fail(new IOException("첨부는 합쳐서 100개까지 선택하세요"));return;}Intent intent;if(photos&&Build.VERSION.SDK_INT>=33){intent=new Intent(MediaStore.ACTION_PICK_IMAGES).setType("image/*");intent.putExtra(MediaStore.EXTRA_PICK_IMAGES_MAX,Math.min(MAX_ATTACHMENTS-attachments.size(),MediaStore.getPickImagesMaxLimit()));}else{intent=new Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType(photos?"image/*":"*/*");intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE,true);}try{startActivityForResult(intent,photos?PICK_PHOTOS:PICK_FILES);}catch(Exception e){fail(e);}}
    private void capture(){try{if(attachments.size()>=MAX_ATTACHMENTS)throw new IOException("첨부는 합쳐서 100개까지 선택하세요");cameraToken=UUID.randomUUID()+".jpg";preferences.edit().putString("cameraToken",cameraToken).apply();Uri uri=CameraOutput.uri(cameraToken);Intent intent=new Intent(MediaStore.ACTION_IMAGE_CAPTURE).putExtra(MediaStore.EXTRA_OUTPUT,uri).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION);intent.setClipData(ClipData.newRawUri("FTA camera",uri));startActivityForResult(intent,CAPTURE);}catch(Exception e){fail(e);}}
    @Override protected void onActivityResult(int request,int result,Intent data){super.onActivityResult(request,result,data);
        if(request==OPEN_RECEIVED){if(result==RESULT_OK&&data!=null&&data.getData()!=null)try{Uri uri=data.getData();String type=getContentResolver().getType(uri);startActivity(new Intent(Intent.ACTION_VIEW).setDataAndType(uri,type==null?"application/octet-stream":type).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION));}catch(Exception e){fail(e);}return;}
        if(request==CAPTURE){String token=cameraToken;cameraToken=null;preferences.edit().remove("cameraToken").apply();if(token==null)return;Uri uri=CameraOutput.uri(token);File captured=new File(getNoBackupFilesDir(),"engine/Outgoing/Camera/"+token);if(result==RESULT_OK&&captured.isFile()&&captured.length()>0)importUris(Collections.singletonList(uri),()->{revokeUriPermission(uri,Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION);new File(NativeBridge.outgoing,"Camera/"+token).delete();});else{revokeUriPermission(uri,Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION);captured.delete();if(result==RESULT_OK)fail(new IOException("촬영한 사진을 저장하지 못했습니다"));}return;}
        if((request==PICK_PHOTOS||request==PICK_FILES)&&result==RESULT_OK&&data!=null){ArrayList<Uri> uris=new ArrayList<>();if(data.getClipData()!=null)for(int i=0;i<data.getClipData().getItemCount();i++)uris.add(data.getClipData().getItemAt(i).getUri());else if(data.getData()!=null)uris.add(data.getData());importUris(uris,null);}
    }
    // Provider grants can disappear after selection; prepare durable private copies before the Rust source grant.
    private void importUris(List<Uri> uris,Runnable cleanup){if(!ready){deferredUris=new ArrayList<>(uris);deferredCleanup=cleanup;return;}if(importing||sending){if(cleanup!=null)cleanup.run();return;}if(attachments.size()+uris.size()>MAX_ATTACHMENTS){if(cleanup!=null)cleanup.run();fail(new IOException("첨부는 합쳐서 100개까지 선택하세요"));return;}importing=true;render();WORK.execute(()->{ArrayList<File> copied=new ArrayList<>();File session=new File(NativeBridge.outgoing,UUID.randomUUID().toString());try{
        for(Uri uri:uris){String filename="파일";try(android.database.Cursor cursor=getContentResolver().query(uri,new String[]{OpenableColumns.DISPLAY_NAME},null,null,null)){if(cursor!=null&&cursor.moveToFirst())filename=cursor.getString(0);}
            filename=filename==null?"파일":filename.replaceAll("[\\\\/\\p{Cntrl}]","_");if(filename.isEmpty()||filename.equals(".")||filename.equals(".."))filename="파일";
            File slot=new File(session,UUID.randomUUID().toString());if(!slot.mkdirs())throw new IOException("임시 폴더 생성 실패");File target=new File(slot,filename);
            try(InputStream input=getContentResolver().openInputStream(uri);FileOutputStream output=new FileOutputStream(target)){if(input==null)throw new IOException("선택한 파일을 열 수 없습니다");byte[] buffer=new byte[64*1024];int n;while((n=input.read(buffer))!=-1){if(destroyed)throw new IOException("파일 준비가 중단됐습니다");if(n>0)output.write(buffer,0,n);}output.getFD().sync();}copied.add(target);
        }
        main.post(()->{if(destroyed){deleteTree(session);return;}attachments.addAll(copied);importing=false;persistDraft();render();});
    }catch(Exception e){deleteTree(session);main.post(()->{importing=false;render();});fail(e);}finally{if(cleanup!=null)main.post(cleanup);}});}
    private void restoreAttachments(){try{JSONArray saved=new JSONArray(preferences.getString("attachments","[]"));for(int i=0;i<saved.length();i++){File f=new File(saved.getString(i));if(f.isFile()&&f.getCanonicalPath().equals(f.getAbsolutePath())&&f.getCanonicalPath().startsWith(NativeBridge.outgoing.getCanonicalPath()+File.separator))attachments.add(f);}}catch(Exception ignored){} }
    private void persistDraft(){JSONArray paths=new JSONArray();for(File file:attachments)paths.put(file.getAbsolutePath());preferences.edit().putString("messageDraft",messageDraft).putString("attachments",paths.toString()).apply();}
    private void clearAttachments(){for(File f:attachments){File parent=f.getParentFile();if(parent!=null&&parent.getParentFile()!=null&&parent.getParentFile().getParentFile()!=null&&parent.getParentFile().getParentFile().equals(NativeBridge.outgoing))deleteTree(parent.getParentFile());}attachments.clear();persistDraft();}
    // A killed import may leave private copies; preserve live drafts/camera results and remove only unreferenced staging.
    private void pruneStaging(){File[] sessions=NativeBridge.outgoing.listFiles();if(sessions==null)return;
        for(File session:sessions){if("Camera".equals(session.getName())){Set<String> keep=new HashSet<>();if(cameraToken!=null)keep.add(cameraToken);if(deferredUris!=null)for(Uri uri:deferredUris)if("app.fta.android.camera".equals(uri.getAuthority()))keep.add(uri.getLastPathSegment());File[] photos=session.listFiles();if(photos!=null)for(File photo:photos)if(!keep.contains(photo.getName()))deleteTree(photo);continue;}
            boolean keep=false;for(File draft:attachments)if(draft.getAbsolutePath().startsWith(session.getAbsolutePath()+File.separator)){keep=true;break;}if(!keep)deleteTree(session);
        }
    }
    private static void deleteTree(File root){try{if(!root.getCanonicalPath().equals(root.getAbsolutePath())){root.delete();return;}}catch(IOException denied){return;}File[] children=root.listFiles();if(children!=null)for(File f:children)deleteTree(f);root.delete();}
    // Capture this attempt's draft. A stored receipt clears only unchanged text, preserving edits made while waiting.
    private void send(boolean files){if(!running||sending||importing||selected.isEmpty()||(files?attachments.isEmpty():messageDraft.trim().isEmpty()))return;
        final String id=UUID.randomUUID().toString(),peer=selected,bodyText=messageDraft;final ArrayList<File> paths=new ArrayList<>(attachments);activeId=id;activePeer=peer;completed.remove(peer);fraction=Double.NaN;sending=true;render();
        WORK.execute(()->{try{JSONObject params=new JSONObject().put("transfer_id",id).put("to",peer).put("kind",files?"files":messageKind(bodyText));if(files){JSONArray array=new JSONArray();for(File f:paths)array.put(f.getCanonicalPath());params.put("paths",array);}else params.put("body",bodyText);
            JSONObject result=(JSONObject)NativeBridge.call("transfers.send_discovered",params);main.post(()->{if(!id.equals(activeId))return;String state=result.optString("state");if("completed".equals(state)){Receipt receipt=new Receipt(id);completed.put(peer,receipt);main.postDelayed(()->{Receipt current=completed.get(peer);if(current!=null&&current.id.equals(id)){completed.remove(peer);renderPeers();}},COMPLETION_MS);if(files)clearAttachments();else if(messageDraft.equals(bodyText)){messageDraft="";if(message!=null)message.setText("");}}else fail(new IOException(result.isNull("error")?stateText(state):result.optString("error")));finishSend(id);});
        }catch(Exception e){main.post(()->finishSend(id));fail(e);}});
    }
    static String messageKind(String body){String trimmed=body.trim();if(trimmed.isEmpty()||trimmed.matches("(?s).*\\s.*"))return "text";try{URI uri=new URI(trimmed);String scheme=uri.getScheme();return ("http".equalsIgnoreCase(scheme)||"https".equalsIgnoreCase(scheme))&&uri.getHost()!=null?"url":"text";}catch(Exception e){return "text";}}
    // Two bounded dim/recover cycles are measured from the stored receipt, never from a view redraw.
    static float completionOpacity(long deadline,long now,boolean animate){
        if(!animate)return 1;long elapsed=now-(deadline-COMPLETION_MS);
        if(elapsed<0||elapsed>=2*BLINK_CYCLE_MS)return 1;
        float phase=(elapsed%BLINK_CYCLE_MS)/(float)BLINK_CYCLE_MS;return 1-0.75f*(1-Math.abs(2*phase-1));
    }
    private void finishSend(String id){if(!id.equals(activeId))return;activeId=null;activePeer=null;fraction=Double.NaN;sending=false;persistDraft();render();loadHistory();}
    private void cancel(){String id=activeId;if(id!=null)CONTROL.execute(()->{try{NativeBridge.call("transfers.cancel",new JSONObject().put("transfer_id",id));}catch(Exception e){fail(e);}});}

    private void renderHistory(){if(historyRows==null)return;
        // Native list rows retain stable accessibility focus when engine polling returns unchanged records.
        String snapshot=history.toString();if(snapshot.equals(renderedHistory))return;renderedHistory=snapshot;historyRows.removeAllViews();
        if(history.length()==0){TextView hint=label("전송 기록이 없습니다",16);hint.setTextColor(textSecondary);hint.setPadding(0,dp(24),0,dp(24));historyRows.addView(hint);}
        for(int i=0;i<history.length();i++){JSONObject record=history.optJSONObject(i);if(record==null)continue;
            String summary=("receive".equals(record.optString("direction"))?"수신":"송신")+" · "+stateText(record.optString("state"))+"\n"+android.text.format.DateUtils.formatDateTime(this,record.optLong("created_at")*1000,android.text.format.DateUtils.FORMAT_SHOW_DATE|android.text.format.DateUtils.FORMAT_SHOW_TIME);
            View row=settingsRow(title(record),summary,"files".equals(record.optString("kind"))?R.drawable.ic_file:R.drawable.ic_history,()->openReceipt(record.optString("transfer_id")));
            row.setOnLongClickListener(v->{new AlertDialog.Builder(this).setTitle("전송 기록을 삭제할까요?").setMessage("최종 수신 파일은 유지됩니다.").setNegativeButton("취소",null).setPositiveButton("기록 삭제",(d,w)->CONTROL.execute(()->{try{NativeBridge.call("inbox.delete",new JSONObject().put("transfer_id",record.optString("transfer_id")));main.post(this::loadHistory);}catch(Exception e){fail(e);}})).show();return true;});
            historyRows.addView(row,new LinearLayout.LayoutParams(-1,-2));historyRows.addView(divider());
        }
    }
    private static String title(JSONObject record){if("files".equals(record.optString("kind"))){JSONArray items=record.optJSONArray("items");return items!=null&&items.length()>0?items.optJSONObject(0).optString("name")+(items.length()>1?" 외 "+(items.length()-1)+"개":""):"파일";}return "url".equals(record.optString("kind"))?"URL":"텍스트";}
    private static String stateText(String state){switch(state){case "completed":return "완료";case "partial":return "부분 완료";case "failed":return "실패";case "cancelled":return "취소됨";case "unknown":return "결과 확인 필요";case "awaiting_registration":return "상대 등록 승인 대기";default:return state;}}
    private void openReceipt(String id){if(!ready){pendingReceipt=id;return;}WORK.execute(()->{try{JSONObject r=(JSONObject)NativeBridge.call("inbox.get",new JSONObject().put("transfer_id",id));main.post(()->showReceipt(r));}catch(Exception e){fail(e);}});}
    private void showReceipt(JSONObject record){if(isFinishing()||destroyed)return;String bodyText=record.isNull("body")?"":record.optString("body","");String text=stateText(record.optString("state"))+"\n"+bodyText;
        JSONArray items=record.optJSONArray("items");if(items!=null)for(int i=0;i<items.length();i++){JSONObject item=items.optJSONObject(i);if(item!=null)text+="\n"+item.optString("name")+" · "+stateText(item.optString("state"));}
        AlertDialog.Builder b=new AlertDialog.Builder(this).setTitle(title(record)).setMessage(text).setNegativeButton("닫기",null);
        if(!bodyText.isEmpty())b.setPositiveButton("복사",(d,w)->{try{getSystemService(android.content.ClipboardManager.class).setPrimaryClip(ClipData.newPlainText("FTA",bodyText));}catch(RuntimeException error){fail(new IOException("클립보드에 복사하지 못했습니다. Android의 본문 크기 제한을 확인하세요."));}});
        if("url".equals(record.optString("kind"))&&"url".equals(messageKind(bodyText)))b.setNeutralButton("링크 열기",(d,w)->{try{startActivity(new Intent(Intent.ACTION_VIEW,Uri.parse(bodyText.trim())));}catch(Exception e){fail(e);}});
        else if(items!=null&&items.length()>0&&"receive".equals(record.optString("direction"))&&"completed".equals(record.optString("state")))b.setNeutralButton("파일 열기",(d,w)->openReceived(items));b.show();}
    private void openReceived(JSONArray items){ArrayList<File> files=new ArrayList<>();ArrayList<String> names=new ArrayList<>();for(int i=0;i<items.length();i++){JSONObject item=items.optJSONObject(i);if(item!=null){String saved=item.optString("saved_name","");if(!saved.isEmpty()){File file=new File(NativeBridge.received,saved);if(file.isFile()){files.add(file);names.add(saved);}}}}if(files.isEmpty()){fail(new IOException("수신 파일을 찾을 수 없습니다"));return;}new AlertDialog.Builder(this).setTitle("파일 열기").setItems(names.toArray(new String[0]),(d,w)->{File file=files.get(w);Uri uri=DocumentsContract.buildDocumentUri("app.fta.android.documents",ReceivedDocuments.documentId(file));try{startActivity(new Intent(Intent.ACTION_VIEW).setDataAndType(uri,ReceivedDocuments.mime(file)).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION));}catch(Exception e){fail(e);}}).show();}
    private void manageDevices(){if(!ready)return;WORK.execute(()->{try{JSONArray devices=(JSONArray)NativeBridge.call("devices.list");main.post(()->{String[] names=new String[devices.length()];for(int i=0;i<names.length;i++){JSONObject p=devices.optJSONObject(i);names[i]=p.optString("display_name")+(p.optBoolean("paired")?" · 등록됨":" · 발견됨");}new AlertDialog.Builder(this).setTitle("등록 해제할 기기").setItems(names,(d,w)->{JSONObject p=devices.optJSONObject(w);if(!p.optBoolean("paired"))return;new AlertDialog.Builder(this).setTitle("기기 등록을 해제할까요?").setMessage(p.optString("display_name")).setNegativeButton("취소",null).setPositiveButton("등록 해제",(a,b)->CONTROL.execute(()->{try{NativeBridge.call("devices.unpair",new JSONObject().put("device_id",p.optString("device_id")));}catch(Exception e){fail(e);}})).show();}).setNegativeButton("닫기",null).show();});}catch(Exception e){fail(e);}});}
    private void deliverNotifications() throws Exception {
        // One event can contain a 1 MiB body; page individually to keep native/JVM control allocations bounded.
        final String consumer="android-ui";JSONObject cursor=(JSONObject)NativeBridge.call("events.cursor",new JSONObject().put("consumer",consumer));JSONObject page=(JSONObject)NativeBridge.call("events.list",new JSONObject().put("after",cursor.optLong("sequence")).put("limit",EVENT_PAGE_SIZE));JSONArray events=page.optJSONArray("events");if(events==null)return;
        NotificationManager manager=getSystemService(NotificationManager.class);manager.createNotificationChannel(new NotificationChannel("received","FTA 수신",NotificationManager.IMPORTANCE_DEFAULT));
        boolean allowed=Build.VERSION.SDK_INT<33||checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)==PackageManager.PERMISSION_GRANTED;
        for(int i=0;i<events.length();i++){JSONObject event=events.optJSONObject(i),record=event.optJSONObject("record");if(record!=null&&"receive".equals(record.optString("direction"))&&allowed){String id=event.optString("transfer_id");Intent intent=new Intent(this,MainActivity.class).setData(Uri.parse("fta://receipt/"+id)).putExtra("transfer_id",id).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);PendingIntent open=PendingIntent.getActivity(this,id.hashCode(),intent,PendingIntent.FLAG_UPDATE_CURRENT|PendingIntent.FLAG_IMMUTABLE);
            manager.notify(id,0,new Notification.Builder(this,"received").setSmallIcon(android.R.drawable.stat_sys_download_done).setContentTitle("FTA 수신 "+stateText(record.optString("state"))).setContentText(title(record)).setContentIntent(open).setAutoCancel(true).build());}
            // Submission/permission suppression advances the DB cursor; it never claims that Android displayed an alert.
            NativeBridge.call("events.ack",new JSONObject().put("consumer",consumer).put("sequence",event.optLong("sequence")));
        }
    }
    // Long license text loads off the UI thread; legal content is local and contains no transfer data.
    private void showLegal(String asset){WORK.execute(()->{try(InputStream input=getAssets().open(asset);ByteArrayOutputStream bytes=new ByteArrayOutputStream()){byte[] buffer=new byte[8192];int n;while((n=input.read(buffer))!=-1)bytes.write(buffer,0,n);String text=bytes.toString(java.nio.charset.StandardCharsets.UTF_8.name());main.post(()->{if(destroyed||isFinishing())return;TextView value=label(text,12);value.setTextIsSelectable(true);value.setPadding(dp(16),dp(8),dp(16),dp(8));ScrollView scroll=new ScrollView(this);scroll.addView(value);new AlertDialog.Builder(this).setTitle(asset.equals("PrivacyPolicy.txt")?"개인정보 처리 안내":"오픈소스 고지").setView(scroll).setPositiveButton("닫기",null).show();});}catch(Exception e){fail(e);}});}
    private void fail(Exception error){main.post(()->{if(destroyed||!visible||isFinishing())return;String text=error.getMessage()==null?"작업을 처리하지 못했습니다":error.getMessage();if(text.equals(lastError))return;lastError=text;new AlertDialog.Builder(this).setTitle("FTA").setMessage(text).setPositiveButton("확인",(d,w)->lastError=null).show();});}
    // Touching outside a text input dismisses the keyboard without intercepting the input's own gestures.
    @Override public boolean dispatchTouchEvent(android.view.MotionEvent event){if(event.getAction()==android.view.MotionEvent.ACTION_DOWN){View focus=getCurrentFocus();if(focus instanceof EditText){Rect bounds=new Rect();focus.getGlobalVisibleRect(bounds);if(!bounds.contains((int)event.getRawX(),(int)event.getRawY())){focus.clearFocus();getSystemService(InputMethodManager.class).hideSoftInputFromWindow(focus.getWindowToken(),0);}}}return super.dispatchTouchEvent(event);}
    private int dp(float x){return Math.round(x*getResources().getDisplayMetrics().density);}
    private TextView label(String text,int size){TextView t=new TextView(this);t.setText(text);t.setTextColor(textPrimary);t.setTextSize(size);return t;}
    private Button button(String text,Runnable action){Button b=new Button(this,null,android.R.attr.borderlessButtonStyle);b.setText(text);b.setAllCaps(false);b.setMinHeight(dp(48));b.setOnClickListener(v->action.run());return b;}
    private void setButtonIcon(Button button,int resource){Drawable icon=getDrawable(resource);icon.setTint(accent);button.setCompoundDrawablesWithIntrinsicBounds(icon,null,null,null);button.setCompoundDrawablePadding(dp(8));button.setPadding(dp(16),0,dp(16),0);button.setGravity(Gravity.START|Gravity.CENTER_VERTICAL);}
    private int themeColor(int attribute){android.content.res.TypedArray values=obtainStyledAttributes(new int[]{attribute});try{return values.getColor(0,Color.BLACK);}finally{values.recycle();}}
    private static int withAlpha(int color,float alpha){return (color&0x00FFFFFF)|(Math.round(255*alpha)<<24);}
    private Drawable selectable(boolean borderless){android.content.res.TypedArray values=obtainStyledAttributes(new int[]{borderless?android.R.attr.selectableItemBackgroundBorderless:android.R.attr.selectableItemBackground});try{return values.getDrawable(0);}finally{values.recycle();}}
    private View divider(){View view=new View(this);view.setBackgroundColor(outline);view.setLayoutParams(new LinearLayout.LayoutParams(-1,Math.max(1,dp(1))));return view;}

    // Draw the desktop's actual 24-unit monitor outline; mobile is a phone and status stays attached to its silhouette.
    private final class IconTile extends View {
        final JSONObject peer;final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
        final RectF tileBounds=new RectF(),phoneBounds=new RectF();
        final Path borderPath=new Path(),progressPath=new Path(),checkPath=new Path();
        final PathMeasure borderMeasure=new PathMeasure();
        IconTile(JSONObject peer){super(MainActivity.this);this.peer=peer;}
        @Override protected void onDraw(Canvas canvas){super.onDraw(canvas);float w=getWidth(),h=getHeight();String id=peer.optString("device_id");boolean active=id.equals(activePeer)&&activeId!=null;
            Receipt receipt=completed.get(id);long now=SystemClock.elapsedRealtime();boolean success=receipt!=null&&now<receipt.until;
            float opacity=success?completionOpacity(receipt.until,now,ValueAnimator.areAnimatorsEnabled()):1;if(success&&ValueAnimator.areAnimatorsEnabled())postInvalidateOnAnimation();
            canvas.saveLayerAlpha(0,0,w,h,Math.round(opacity*255));RectF tile=tileBounds;tile.set(dp(2),dp(2),w-dp(2),h-dp(2));paint.setStyle(Paint.Style.FILL);paint.setColor(surface);canvas.drawRoundRect(tile,dp(12),dp(12),paint);
            paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth(dp(active?3:2));paint.setColor(active?outline:id.equals(selected)?accent:outline);canvas.drawRoundRect(tile,dp(12),dp(12),paint);
            if(active&&!Double.isNaN(fraction)){Path border=borderPath,arc=progressPath;border.reset();arc.reset();border.addRoundRect(tile,dp(12),dp(12),Path.Direction.CW);PathMeasure measure=borderMeasure;measure.setPath(border,false);measure.getSegment(0,(float)(measure.getLength()*fraction),arc,true);paint.setColor(transferGreen);paint.setStrokeCap(Paint.Cap.ROUND);canvas.drawPath(arc,paint);paint.setStrokeCap(Paint.Cap.BUTT);}
            float side=dp(34),x=(w-side)/2,y=(h-side)/2;paint.setColor(textPrimary);paint.setStrokeWidth(dp(1.7f));boolean mobile="android".equals(peer.optString("os"))||"ios".equals(peer.optString("os"));
            if(mobile){phoneBounds.set(x+side*.28f,y+side*.04f,x+side*.72f,y+side*.96f);canvas.drawRoundRect(phoneBounds,dp(3),dp(3),paint);canvas.drawLine(x+side*.44f,y+side*.15f,x+side*.56f,y+side*.15f,paint);}else{canvas.drawRect(x+side*3/24,y+side*4/24,x+side*21/24,y+side*16/24,paint);canvas.drawLine(x+side/2,y+side*16/24,x+side/2,y+side*21/24,paint);canvas.drawLine(x+side*8/24,y+side*21/24,x+side*16/24,y+side*21/24,paint);}canvas.restore();
            paint.setStyle(Paint.Style.FILL);paint.setColor(transferGreen);canvas.drawCircle(x+side*(mobile?.74f:.89f),y+side*(mobile?.9f:.67f),dp(4),paint);
            if(success){float cx=w-dp(11),cy=h-dp(11);paint.setColor(transferGreen);canvas.drawCircle(cx,cy,dp(11),paint);paint.setColor(completionInk);paint.setStyle(Paint.Style.STROKE);paint.setStrokeWidth(dp(2));Path check=checkPath;check.reset();check.moveTo(cx-dp(5),cy);check.lineTo(cx-dp(1),cy+dp(4));check.lineTo(cx+dp(6),cy-dp(4));canvas.drawPath(check,paint);}
        }
    }
}
