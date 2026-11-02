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
    private static final int MAX_ATTACHMENTS=100;
    private static final long COMPLETION_MS=1500, BLINK_CYCLE_MS=600;
    // Lifecycle controls are serialized globally, so recreation cannot stop a newer listener from an old worker.
    private static final ExecutorService CONTROL=TransferService.CONTROL;
    private static final ExecutorService WORK=Executors.newFixedThreadPool(3);
    private final Handler main=new Handler(Looper.getMainLooper());
    private SharedPreferences preferences;
    private TransferService session;
    private boolean bound,prepared,stateReceiverRegistered;
    private final BroadcastReceiver stateReceiver=new BroadcastReceiver(){public void onReceive(Context context,Intent intent){updateLifecycle();}};
    private final String clientToken=UUID.randomUUID().toString();
    private final ServiceConnection connection=new ServiceConnection(){
        public void onServiceConnected(ComponentName name,IBinder binder){if(!visible||destroyed)return;session=((TransferService.LocalBinder)binder).service();session.attach(clientToken);updateLifecycle();}
        public void onServiceDisconnected(ComponentName name){session=null;running=false;ready=false;if(visible)render();}
    };
    private volatile boolean visible,ready,destroyed;
    private String selected="",preferredPeer="",activeId,activePeer,messageDraft="",cameraToken,lastOutcomeSeen;
    private boolean running,sending,importing;
    private double fraction=Double.NaN;
    private boolean displayedReceive=true;
    // Only a visible Activity renders snapshots; the Service retains no view listener across Home or recreation.
    private final Runnable viewTick=new Runnable(){public void run(){if(!visible||destroyed)return;updateLifecycle();main.postDelayed(this,750);}};
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
    private ScrollView pageScroll;
    private boolean restoringScroll;
    private java.util.concurrent.atomic.AtomicBoolean galleryCancellation;
    private boolean savingGallery;
    private AlertDialog galleryProgress;
    private String pendingGalleryResult;

    private int[] scrollOffsets=new int[3];
    private LinearLayout navigation;
    private MenuItem refreshAction;
    private int background,surface,textPrimary,textSecondary,accent,outline,transferGreen,completionInk;
    private String lastError,pendingReceipt,renderedPeers,renderedHistory,renderedAttachments;
    private List<Uri> deferredUris;
    private Runnable deferredCleanup;
    private static final class Receipt {final String id;final long until;Receipt(String id,long deadline){this.id=id;until=deadline;}}

    @Override public void onCreate(Bundle state){super.onCreate(state);
        // Theme attributes keep all Android controls and custom status drawing consistent with system light/night mode.
        background=themeColor(android.R.attr.colorBackground);surface=themeColor(android.R.attr.colorBackgroundFloating);textPrimary=themeColor(android.R.attr.textColorPrimary);textSecondary=themeColor(android.R.attr.textColorSecondary);accent=themeColor(android.R.attr.colorAccent);outline=withAlpha(textSecondary,0.24f);
        // Green remains semantic progress/success; theme-specific contrast keeps the ring and check legible in both modes.
        boolean night=(getResources().getConfiguration().uiMode&android.content.res.Configuration.UI_MODE_NIGHT_MASK)==android.content.res.Configuration.UI_MODE_NIGHT_YES;transferGreen=night?0xFF81C995:0xFF188038;completionInk=night?Color.BLACK:WHITE;
        preferences=getSharedPreferences("fta",MODE_PRIVATE);messageDraft=preferences.getString("messageDraft","");cameraToken=preferences.getString("cameraToken",null);
        getWindow().setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        if(state!=null&&state.getIntArray("scrollOffsets")!=null)scrollOffsets=state.getIntArray("scrollOffsets");
        // Recover only this app’s unfinished MediaStore rows; normal received files and published gallery copies stay intact.
        Context appContext=getApplicationContext();WORK.execute(()->GalleryExport.recover(appContext));
        createRoot();showTab(state==null?0:state.getInt("selectedTab",0));
        // Cold-start notification navigation waits for actual engine readiness rather than an arbitrary delay.
        pendingReceipt=getIntent().getStringExtra("transfer_id");
    }
    // Pre-33 receivers use a signature permission because the non-exported flag is unavailable on those OS versions.
    @android.annotation.SuppressLint("UnspecifiedRegisterReceiverFlag")
    @Override protected void onStart(){super.onStart();visible=true;if(Build.VERSION.SDK_INT>=33)registerReceiver(stateReceiver,new IntentFilter(TransferService.STATE_CHANGED),Context.RECEIVER_NOT_EXPORTED);else registerReceiver(stateReceiver,new IntentFilter(TransferService.STATE_CHANGED),TransferService.STATE_PERMISSION,null);stateReceiverRegistered=true;bound=bindService(new Intent(this,TransferService.class),connection,BIND_AUTO_CREATE);main.post(viewTick);}
    @Override protected void onResume(){super.onResume();if(tab==2&&body!=null)showTab(2);}
    @Override protected void onStop(){visible=false;persistDraft();main.removeCallbacks(viewTick);if(stateReceiverRegistered){unregisterReceiver(stateReceiver);stateReceiverRegistered=false;}if(session!=null)session.detach(clientToken);session=null;if(bound){unbindService(connection);bound=false;}super.onStop();}
    @Override protected void onDestroy(){destroyed=true;if(galleryCancellation!=null)galleryCancellation.set(true);if(galleryProgress!=null)galleryProgress.dismiss();main.removeCallbacksAndMessages(null);super.onDestroy();}
    @Override protected void onNewIntent(Intent intent){super.onNewIntent(intent);setIntent(intent);String id=intent.getStringExtra("transfer_id");if(id!=null)openReceipt(id);}
    @Override public void onRequestPermissionsResult(int request,String[] permissions,int[] grants){super.onRequestPermissionsResult(request,permissions,grants);if(request==LAN_PERMISSION){updateLifecycle();showTab(tab);} }

    private boolean permitted(){return Build.VERSION.SDK_INT<37||checkSelfPermission("android.permission.ACCESS_LOCAL_NETWORK")==PackageManager.PERMISSION_GRANTED;}
    private boolean trusted(LanDiscovery.Lan lan){return lan!=null&&preferences.getStringSet("trustedScopes",Collections.emptySet()).contains(lan.scope);}
    private String name(){return preferences.getString("deviceName",Build.MODEL);}
    // Binding exposes state only. Starting/stopping sockets and send workers belongs exclusively to the Service.
    private void updateLifecycle(){
        consumeGalleryResult();
        if(destroyed||!visible||session==null)return;session.fromUi();ready=session.ready;running=session.running;sending=session.sending;activeId=session.activeId;activePeer=session.activePeer;fraction=session.fraction;peers=session.peers;history=session.history;registrations=session.registrations;
        completed.entrySet().removeIf(entry->!session.completed.containsKey(entry.getKey()));for(Map.Entry<String,TransferService.Completion> entry:session.completed.entrySet()){Receipt previous=completed.get(entry.getKey());if(previous==null||!previous.id.equals(entry.getValue().id))completed.put(entry.getKey(),new Receipt(entry.getValue().id,entry.getValue().until));}
        if(ready&&!prepared){prepared=true;restoreAttachments();pruneStaging();if(pendingReceipt!=null){String receipt=pendingReceipt;pendingReceipt=null;openReceipt(receipt);}if(deferredUris!=null){List<Uri> uris=deferredUris;Runnable cleanup=deferredCleanup;deferredUris=null;deferredCleanup=null;importUris(uris,cleanup);}}
        applyOutcome();
        if(session.lastError!=null){String error=session.lastError;session.lastError=null;fail(new IOException(error));}
        boolean receive=preferences.getBoolean("receiveEnabled",true);if(tab==2&&receive!=displayedReceive)showTab(2);displayedReceive=receive;render();presentRegistration();
    }
    private void applyOutcome(){if(session==null)return;if(session.lastOutcome!=null&&!session.lastOutcome.equals(lastOutcomeSeen)){lastOutcomeSeen=session.lastOutcome;if("completed".equals(session.lastOutcomeState)){if(session.lastOutcomeFiles){attachments.clear();restoreAttachments();}else if(messageDraft.equals(session.lastOutcomeBody)){messageDraft=preferences.getString("messageDraft","");if(message!=null)message.setText(messageDraft);}}}}
    private void poll(){updateLifecycle();}
    private void loadHistory(){if(session!=null)session.refreshHistory();}
    static double fileFraction(JSONObject transfer){if(!"files".equals(transfer.optString("kind")))return Double.NaN;JSONArray items=transfer.optJSONArray("items");double total=0,sent=0;if(items!=null)for(int i=0;i<items.length();i++){JSONObject item=items.optJSONObject(i);if(item!=null){total+=item.optDouble("size",0);sent+=item.optDouble("bytes_transferred",0);}}return total>0?Math.min(1,Math.max(0,sent/total)):Double.NaN;}
    // Complete formatted messages and quantities let each locale order labels without translating payloads or JSON keys.
    private void presentRegistration(){if(!visible||isFinishing())return;for(int i=0;i<registrations.length();i++){JSONObject request=registrations.optJSONObject(i);if(request==null)continue;String id=request.optString("session_id");if(!shownRequests.add(id))continue;
        new AlertDialog.Builder(this).setTitle(getString(R.string.device_registration_request)).setMessage(getString(R.string.registration_summary,request.optString("display_name"),request.optString("os"),kindText(request.optString("kind")),getResources().getQuantityString(R.plurals.item_count,request.optInt("item_count"),request.optInt("item_count"))))
            .setPositiveButton(getString(R.string.register_device),(d,w)->registration(id,true)).setNegativeButton(getString(R.string.decline),(d,w)->registration(id,false)).setOnCancelListener(d->registration(id,false)).show();break;}}
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
        refreshAction=toolbar.getMenu().add(getString(R.string.refresh_devices));Drawable refresh=getDrawable(R.drawable.ic_refresh);refresh.setTint(textPrimary);refreshAction.setIcon(refresh);refreshAction.setShowAsAction(MenuItem.SHOW_AS_ACTION_ALWAYS);
        toolbar.setOnMenuItemClickListener(item->{if(tab==1)loadHistory();else if(running)poll();else updateLifecycle();return true;});
        body=new LinearLayout(this);body.setOrientation(LinearLayout.VERTICAL);root.addView(body,new LinearLayout.LayoutParams(-1,0,1));
        navigation=new LinearLayout(this);navigation.setOrientation(LinearLayout.VERTICAL);navigation.setBackgroundColor(surface);navigation.addView(divider());
        RadioGroup tabs=new RadioGroup(this);tabs.setOrientation(LinearLayout.HORIZONTAL);tabs.setGravity(Gravity.CENTER_VERTICAL);
        String[] titles={getString(R.string.send),getString(R.string.history),getString(R.string.settings)};int[] icons={R.drawable.ic_send,R.drawable.ic_history,R.drawable.ic_settings};
        android.content.res.ColorStateList colors=new android.content.res.ColorStateList(new int[][]{new int[]{android.R.attr.state_checked},new int[]{}},new int[]{accent,textSecondary});
        for(int i=0;i<titles.length;i++){final int index=i;RadioButton button=new RadioButton(this);button.setId(View.generateViewId());button.setButtonDrawable(null);button.setText(titles[i]);button.setTextSize(12);button.setGravity(Gravity.CENTER);button.setPadding(0,dp(8),0,dp(8));button.setTextColor(colors);button.setCompoundDrawablesWithIntrinsicBounds(null,getDrawable(icons[i]),null,null);button.setCompoundDrawableTintList(colors);button.setCompoundDrawablePadding(dp(4));button.setBackground(selectable(false));button.setOnClickListener(v->showTab(index));tabButtons[i]=button;tabs.addView(button,new RadioGroup.LayoutParams(0,dp(64),1));}
        navigation.addView(tabs);root.addView(navigation);setContentView(root);
    }
    @Override protected void onSaveInstanceState(Bundle state){state.putInt("selectedTab",tab);if(pageScroll!=null&&!restoringScroll)scrollOffsets[tab]=pageScroll.getScrollY();state.putIntArray("scrollOffsets",scrollOffsets);super.onSaveInstanceState(state);}
    private void showTab(int value){
        if(pageScroll!=null&&!restoringScroll)scrollOffsets[tab]=pageScroll.getScrollY();
        if(message!=null)messageDraft=message.getText().toString();tab=value;
        for(int i=0;i<tabButtons.length;i++)if(tabButtons[i]!=null){tabButtons[i].setChecked(i==value);tabButtons[i].setTypeface(null,i==value?Typeface.BOLD:Typeface.NORMAL);}
        toolbar.setTitle(value==0?"FTA":value==1?getString(R.string.transfer_history):getString(R.string.settings));toolbar.setSubtitle(value==0?"File Transfer Anything":null);refreshAction.setVisible(value!=2);refreshAction.setTitle(value==1?getString(R.string.refresh_history):getString(R.string.refresh_devices));
        // Move the same header between page containers; rebuilding settings must not leave a duplicate or fixed spacer.
        if(toolbar.getParent() instanceof ViewGroup)((ViewGroup)toolbar.getParent()).removeView(toolbar);
        body.removeAllViews();pageScroll=null;message=null;status=null;deviceRow=null;historyRows=null;
        if(tab==0)buildSend();else if(tab==1)buildHistory();else buildSettings();render();
    }
    private LinearLayout column(){LinearLayout c=new LinearLayout(this);c.setOrientation(LinearLayout.VERTICAL);c.setPadding(dp(16),dp(16),dp(16),dp(16));return c;}
    // Header/body share one viewport; skip transient zero offsets until posted restoration completes, keeping the composer/navigation outside.
    private ScrollView scroll(LinearLayout content){LinearLayout page=new LinearLayout(this);page.setOrientation(LinearLayout.VERTICAL);page.addView(toolbar,new LinearLayout.LayoutParams(-1,dp(56)));page.addView(divider());page.addView(content,new LinearLayout.LayoutParams(-1,-2));ScrollView s=new ScrollView(this);s.setFillViewport(true);s.addView(page,new ScrollView.LayoutParams(-1,-2));pageScroll=s;restoringScroll=true;final int restoreOffset=scrollOffsets[tab];s.post(()->{if(pageScroll!=s)return;s.scrollTo(0,restoreOffset);restoringScroll=false;});return s;}
    private TextView section(String title){TextView text=label(title,14);text.setTypeface(null,Typeface.BOLD);text.setTextColor(accent);text.setPadding(0,dp(20),0,dp(8));return text;}
    private void buildSend(){
        renderedPeers=null;renderedAttachments=null;LinearLayout content=column();body.addView(scroll(content),new LinearLayout.LayoutParams(-1,0,1));
        status=label("",14);status.setTextColor(textSecondary);status.setPadding(0,0,0,dp(8));content.addView(status);content.addView(section(getString(R.string.available_devices)));
        HorizontalScrollView devices=new HorizontalScrollView(this);devices.setHorizontalScrollBarEnabled(false);deviceRow=new LinearLayout(this);deviceRow.setPadding(0,dp(8),0,dp(8));devices.addView(deviceRow);LinearLayout online=group(content);online.setPadding(dp(16),0,dp(16),0);online.addView(devices);
        // Fill the device surface and balance its padding so translated empty text stays centered.
        empty=label(getString(R.string.no_devices_found),16);empty.setTextColor(textSecondary);empty.setGravity(Gravity.CENTER);empty.setPadding(0,dp(32),0,dp(32));online.addView(empty,new LinearLayout.LayoutParams(-1,-2));
        content.addView(section(getString(R.string.photos_files)));LinearLayout actions=new LinearLayout(this);actions.setPadding(0,dp(4),0,dp(4));photoButton=button(getString(R.string.photos),this::photoChoice);fileButton=button(getString(R.string.files),()->pick(false));setButtonIcon(photoButton,R.drawable.ic_photo);setButtonIcon(fileButton,R.drawable.ic_file);LinearLayout.LayoutParams photoParams=new LinearLayout.LayoutParams(0,dp(56),1);photoParams.setMarginEnd(dp(8));actions.addView(photoButton,photoParams);actions.addView(fileButton,new LinearLayout.LayoutParams(0,dp(56),1));content.addView(actions);
        attachmentText=label("",14);attachmentText.setTextColor(textSecondary);attachmentText.setPadding(0,dp(8),0,0);content.addView(attachmentText);attachmentRow=new LinearLayout(this);attachmentRow.setOrientation(LinearLayout.VERTICAL);content.addView(attachmentRow);
        // A bounded, filled input distinguishes editable content from Settings-style rows while retaining native selection and scrolling.
        LinearLayout composer=new LinearLayout(this);composer.setOrientation(LinearLayout.VERTICAL);composer.setBackgroundColor(surface);composer.addView(divider());
        LinearLayout row=new LinearLayout(this);row.setGravity(Gravity.BOTTOM);row.setPadding(dp(16),dp(8),dp(8),dp(8));
        message=new EditText(this);message.setText(messageDraft);message.setHint(getString(R.string.text_or_url));message.setTextSize(16);message.setInputType(android.text.InputType.TYPE_CLASS_TEXT|android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE|android.text.InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);message.setMinLines(1);message.setMaxLines(4);message.setVerticalScrollBarEnabled(true);message.setMinHeight(dp(48));message.setPadding(dp(12),dp(10),dp(12),dp(10));message.setBackground(inputSurface());
        message.addTextChangedListener(new TextWatcher(){public void beforeTextChanged(CharSequence s,int st,int count,int after){}public void onTextChanged(CharSequence s,int st,int before,int count){messageDraft=s.toString();preferences.edit().putString("messageDraft",messageDraft).apply();renderActions();}public void afterTextChanged(Editable e){}});
        row.addView(message,new LinearLayout.LayoutParams(0,-2,1));sendButton=new ImageButton(this);sendButton.setImageResource(R.drawable.ic_send);sendButton.setImageTintList(android.content.res.ColorStateList.valueOf(actionInk()));sendButton.setPadding(dp(12),dp(12),dp(12),dp(12));sendButton.setContentDescription(getString(R.string.send_message));sendButton.setBackground(actionSurface(accent));sendButton.setOnClickListener(v->send(false));LinearLayout.LayoutParams sendParams=new LinearLayout.LayoutParams(dp(48),dp(48));sendParams.setMarginStart(dp(8));row.addView(sendButton,sendParams);composer.addView(row);body.addView(composer,new LinearLayout.LayoutParams(-1,-2));
    }
    private void buildHistory(){renderedHistory=null;LinearLayout content=column();historyRows=new LinearLayout(this);historyRows.setOrientation(LinearLayout.VERTICAL);group(content).addView(historyRows);body.addView(scroll(content),new LinearLayout.LayoutParams(-1,-1));loadHistory();}
    // Grouped value rows match Android Settings and make switches, navigation and current settings distinct.
    private void buildSettings(){
        LinearLayout c=column();status=label("",14);status.setTextColor(textSecondary);c.addView(status);c.addView(section(getString(R.string.network_section)));LinearLayout networkGroup=group(c);
        LanDiscovery.Lan lan=LanDiscovery.current(this);
        Switch trust=new Switch(this);trust.setText(getString(R.string.i_trust_this_home_network));trust.setTextSize(16);trust.setMinHeight(dp(48));trust.setChecked(trusted(lan));trust.setEnabled(lan!=null);trust.setOnCheckedChangeListener((b,on)->{Set<String> scopes=new HashSet<>(preferences.getStringSet("trustedScopes",Collections.emptySet()));if(lan!=null){if(on)scopes.add(lan.scope);else scopes.remove(lan.scope);}preferences.edit().putStringSet("trustedScopes",scopes).apply();if(on&&!permitted())requestPermissions(new String[]{"android.permission.ACCESS_LOCAL_NETWORK"},LAN_PERMISSION);updateLifecycle();});
        switchRow(networkGroup,trust,lan==null?getString(R.string.connect_lan):lan.interfaceName+" · "+lan.cidr);networkGroup.addView(divider());
        Switch receive=new Switch(this);receive.setText(getString(R.string.enable_sending_and_receiving));receive.setTextSize(16);receive.setMinHeight(dp(48));receive.setChecked(preferences.getBoolean("receiveEnabled",true));receive.setOnCheckedChangeListener((b,on)->{preferences.edit().putBoolean("receiveEnabled",on).apply();updateLifecycle();});switchRow(networkGroup,receive,getString(R.string.background_summary));
        c.addView(section(getString(R.string.devices_section)));LinearLayout deviceGroup=group(c);
        deviceGroup.addView(settingsRow(getString(R.string.device_name),name(),R.drawable.ic_info,this::editDeviceName));deviceGroup.addView(divider());deviceGroup.addView(settingsRow(getString(R.string.manage_devices),getString(R.string.unregister_summary),R.drawable.ic_network,this::manageDevices));
        c.addView(section(getString(R.string.files_permissions)));LinearLayout files=group(c);files.addView(settingsRow(getString(R.string.open_received),getString(R.string.files_app_summary),R.drawable.ic_file,()->{try{Intent intent=new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").addCategory(Intent.CATEGORY_OPENABLE).putExtra(DocumentsContract.EXTRA_INITIAL_URI,DocumentsContract.buildRootUri("app.fta.android.documents","received"));startActivityForResult(intent,OPEN_RECEIVED);}catch(Exception e){fail(e);}}));files.addView(divider());
        files.addView(settingsRow(getString(R.string.allow_notifications),(getSystemService(NotificationManager.class).areNotificationsEnabled()?getString(R.string.allowed):getString(R.string.not_allowed))+getString(R.string.notification_suffix),R.drawable.ic_notifications,()->{if(Build.VERSION.SDK_INT>=33&&checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)!=PackageManager.PERMISSION_GRANTED)requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS},NOTIFICATIONS);else startActivity(new Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE,getPackageName()));}));files.addView(divider());
        files.addView(settingsRow(getString(R.string.lan_permission_settings),(Build.VERSION.SDK_INT<37?getString(R.string.permission_not_required):permitted()?getString(R.string.allowed):getString(R.string.not_allowed))+getString(R.string.app_permission_suffix),R.drawable.ic_network,()->startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS,Uri.parse("package:"+getPackageName())))));
        c.addView(section(getString(R.string.about_section)));LinearLayout info=group(c);info.addView(settingsRow(getString(R.string.privacy_policy),null,R.drawable.ic_info,()->showLegal("PrivacyPolicy.txt")));info.addView(divider());info.addView(settingsRow(getString(R.string.open_source_notices),null,R.drawable.ic_info,()->showLegal("ThirdPartyNotices.txt")));TextView version=label(getString(R.string.app_version,BuildConfig.VERSION_NAME),12);version.setTextColor(textSecondary);version.setPadding(dp(16),dp(16),0,0);c.addView(version);body.addView(scroll(c),new LinearLayout.LayoutParams(-1,-1));
    }
    // Settings-style rows expose the current value and edit in a native dialog instead of an always-open form.
    private void editDeviceName(){
        EditText editor=new EditText(this);editor.setText(name());editor.setHint(getString(R.string.device_name));editor.setSingleLine(true);editor.setContentDescription(getString(R.string.device_name));LinearLayout box=new LinearLayout(this);box.setPadding(dp(24),dp(8),dp(24),0);box.addView(editor,new LinearLayout.LayoutParams(-1,-2));AlertDialog dialog=new AlertDialog.Builder(this).setTitle(getString(R.string.device_name)).setView(box).setNegativeButton(getString(R.string.cancel),null).setPositiveButton(getString(R.string.save),null).create();dialog.setOnShowListener(d->dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(v->{String value=editor.getText().toString().trim();if(value.isEmpty()||value.getBytes(java.nio.charset.StandardCharsets.UTF_8).length>128){editor.setError(getString(R.string.name_byte_limit));return;}preferences.edit().putString("deviceName",value).apply();updateLifecycle();dialog.dismiss();showTab(2);}));dialog.show();
    }
    private void switchRow(LinearLayout group,Switch control,String summary){LinearLayout row=new LinearLayout(this);row.setOrientation(LinearLayout.VERTICAL);row.setPadding(dp(16),dp(8),dp(16),dp(16));row.addView(control,new LinearLayout.LayoutParams(-1,-2));TextView detail=label(summary,14);detail.setTextColor(textSecondary);detail.setPadding(0,dp(4),dp(48),0);row.addView(detail);group.addView(row);}
    private View settingsRow(String title,String summary,int icon,Runnable action){
        LinearLayout row=new LinearLayout(this);row.setGravity(Gravity.CENTER_VERTICAL);row.setPadding(dp(16),dp(16),dp(16),dp(16));row.setMinimumHeight(dp(76));row.setBackground(selectable(false));row.setOnClickListener(v->action.run());row.setFocusable(true);
        ImageView image=new ImageView(this);image.setImageResource(icon);image.setImageTintList(android.content.res.ColorStateList.valueOf(accent));image.setPadding(dp(8),dp(8),dp(8),dp(8));GradientDrawable badge=new GradientDrawable();badge.setShape(GradientDrawable.OVAL);badge.setColor(withAlpha(accent,0.12f));image.setBackground(badge);LinearLayout.LayoutParams ip=new LinearLayout.LayoutParams(dp(40),dp(40));ip.setMarginEnd(dp(16));row.addView(image,ip);
        LinearLayout labels=new LinearLayout(this);labels.setOrientation(LinearLayout.VERTICAL);labels.addView(label(title,16));if(summary!=null){TextView detail=label(summary,14);detail.setTextColor(textSecondary);detail.setPadding(0,dp(4),0,0);labels.addView(detail);}row.addView(labels,new LinearLayout.LayoutParams(0,-2,1));TextView next=label("›",24);next.setTextColor(textSecondary);next.setPadding(dp(12),0,0,0);next.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);row.addView(next);return row;
    }
    // Group surfaces follow the observed Android Settings hierarchy; controls remain real framework widgets.
    private LinearLayout group(LinearLayout parent){LinearLayout group=new LinearLayout(this);group.setOrientation(LinearLayout.VERTICAL);GradientDrawable fill=new GradientDrawable();fill.setColor(surface);fill.setCornerRadius(dp(24));group.setBackground(fill);group.setClipToOutline(true);LinearLayout.LayoutParams p=new LinearLayout.LayoutParams(-1,-2);p.bottomMargin=dp(8);parent.addView(group,p);return group;}
    private android.graphics.drawable.Drawable actionSurface(int fill){GradientDrawable shape=new GradientDrawable();shape.setColor(fill);shape.setCornerRadius(dp(12));shape.setStroke(dp(1),withAlpha(textSecondary,0.3f));return new RippleDrawable(android.content.res.ColorStateList.valueOf(withAlpha(textPrimary,0.12f)),shape,null);}
    private android.graphics.drawable.Drawable inputSurface(){StateListDrawable states=new StateListDrawable();for(boolean focus:new boolean[]{true,false}){GradientDrawable fill=new GradientDrawable();fill.setColor(background);fill.setCornerRadius(dp(12));fill.setStroke(dp(focus?2:1),focus?accent:withAlpha(textSecondary,0.55f));states.addState(focus?new int[]{android.R.attr.state_focused}:new int[]{},fill);}return states;}
    private int actionInk(){return (getResources().getConfiguration().uiMode&android.content.res.Configuration.UI_MODE_NIGHT_MASK)==android.content.res.Configuration.UI_MODE_NIGHT_YES?0xFF102032:Color.WHITE;}
    private void render(){if(destroyed)return;renderStatus();if(tab==0){renderPeers();renderActions();renderAttachments();}else if(tab==1)renderHistory();}
    private void renderStatus(){if(status!=null){status.setText(session==null?getString(R.string.binding_service):session.status());
        // Green means the real LAN transfer service is running, not merely that Wi-Fi is enabled.
        if(tab==0){Drawable radio=getDrawable(R.drawable.ic_radio_status);radio.setTint(running?transferGreen:textSecondary);radio.setBounds(0,0,dp(20),dp(20));status.setCompoundDrawablesRelative(radio,null,null,null);status.setCompoundDrawablePadding(dp(6));status.setGravity(Gravity.CENTER_VERTICAL);}
    }}
    private void renderPeers(){if(deviceRow==null)return;
        // Stable rows preserve native accessibility focus; polling identical engine data must not recreate controls.
        String snapshot=peers.toString()+"|"+selected+"|"+activeId+"|"+fraction+"|"+completed.toString();if(snapshot.equals(renderedPeers))return;renderedPeers=snapshot;
        deviceRow.removeAllViews();boolean exists=false;
        for(int i=0;i<peers.length();i++){JSONObject p=peers.optJSONObject(i);if(!availableTarget(p))continue;String id=p.optString("device_id");if(selected.isEmpty()&&id.equals(preferredPeer))selected=id;if(id.equals(selected))exists=true;
            LinearLayout item=new LinearLayout(this);item.setOrientation(LinearLayout.VERTICAL);item.setGravity(Gravity.CENTER_HORIZONTAL);item.setPadding(0,0,dp(16),0);IconTile icon=new IconTile(p);item.addView(icon,new LinearLayout.LayoutParams(dp(78),dp(76)));TextView title=label(p.optString("display_name"),12);title.setGravity(Gravity.CENTER);title.setMaxLines(2);LinearLayout.LayoutParams tp=new LinearLayout.LayoutParams(dp(90),-2);tp.topMargin=dp(8);item.addView(title,tp);String feedback="";Receipt receipt=completed.get(id);if(receipt!=null&&SystemClock.elapsedRealtime()<receipt.until)feedback=getString(R.string.completed_accessibility);else if(id.equals(activePeer)&&activeId!=null)feedback=Double.isNaN(fraction)?getString(R.string.preparing_accessibility):getString(R.string.progress_accessibility,(int)(fraction*100));item.setContentDescription(p.optString("display_name")+(p.optBoolean("paired")?getString(R.string.online_accessibility):getString(R.string.unpaired_accessibility))+feedback);item.setSelected(id.equals(selected));item.setOnClickListener(v->{if(sending)return;selected=id;preferredPeer=id;renderPeers();renderActions();});deviceRow.addView(item);
        }
        // The empty scroll row otherwise leaves top-only padding above the centered message.
        if(!exists)selected="";boolean noDevices=deviceRow.getChildCount()==0;((View)deviceRow.getParent()).setVisibility(noDevices?View.GONE:View.VISIBLE);empty.setVisibility(noDevices?View.VISIBLE:View.GONE);
    }
    // Only Service available_only results enter this renderer: unpaired targets already passed TCP reachability,
    // while their online field remains false until registration. Preserve offline filtering for saved registrations.
    static boolean availableTarget(JSONObject peer){return peer!=null&&(!peer.optBoolean("paired")||peer.optBoolean("online"));}
    private void renderActions(){if(tab!=0||sendButton==null)return;boolean can=running&&!selected.isEmpty()&&!sending&&!importing;sendButton.setEnabled(can&&!messageDraft.trim().isEmpty());sendButton.setAlpha(sendButton.isEnabled()?1:0.4f);photoButton.setEnabled(!sending&&!importing&&ready);fileButton.setEnabled(!sending&&!importing&&ready);}
    private void renderAttachments(){if(attachmentText==null)return;
        String snapshot=attachments.toString()+"|"+importing+"|"+sending+"|"+running+"|"+selected;if(snapshot.equals(renderedAttachments))return;renderedAttachments=snapshot;
        attachmentText.setText(importing?getString(R.string.preparing_items):attachments.isEmpty()?"":getResources().getQuantityString(R.plurals.selected_count,attachments.size(),attachments.size()));attachmentRow.removeAllViews();if(!attachments.isEmpty()){
        StringBuilder names=new StringBuilder();for(File file:attachments){if(names.length()>0)names.append("\n");names.append(file.getName());}TextView list=label(names.toString(),12);list.setMaxLines(4);attachmentRow.addView(list);
        LinearLayout buttons=new LinearLayout(this);Button send=button(sending?getString(R.string.cancel):getString(R.string.send_selection),()->{if(sending)cancel();else send(true);});send.setBackground(actionSurface(accent));send.setTextColor(actionInk());send.setEnabled(sending||(running&&!selected.isEmpty()&&!importing));send.setAlpha(send.isEnabled()?1:0.4f);LinearLayout.LayoutParams transferParams=new LinearLayout.LayoutParams(0,-2,1);transferParams.setMarginEnd(dp(8));buttons.addView(send,transferParams);Button clear=button(getString(R.string.clear_selection),()->{clearAttachments();renderAttachments();});clear.setEnabled(!sending&&!importing);clear.setAlpha(clear.isEnabled()?1:0.4f);buttons.addView(clear);attachmentRow.addView(buttons);
    }else if(sending)attachmentRow.addView(button(getString(R.string.cancel_transfer),this::cancel));}

    private void photoChoice(){PopupMenu menu=new PopupMenu(this,photoButton);menu.getMenu().add(getString(R.string.choose_from_photos)).setOnMenuItemClickListener(i->{pick(true);return true;});menu.getMenu().add(getString(R.string.take_photo)).setOnMenuItemClickListener(i->{capture();return true;});menu.show();}
    private void pick(boolean photos){if(!ready)return;if(attachments.size()>=MAX_ATTACHMENTS){fail(new IOException(getString(R.string.attachment_limit)));return;}Intent intent;if(photos&&Build.VERSION.SDK_INT>=33){intent=new Intent(MediaStore.ACTION_PICK_IMAGES).setType("image/*");intent.putExtra(MediaStore.EXTRA_PICK_IMAGES_MAX,Math.min(MAX_ATTACHMENTS-attachments.size(),MediaStore.getPickImagesMaxLimit()));}else{intent=new Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType(photos?"image/*":"*/*");intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE,true);}try{startActivityForResult(intent,photos?PICK_PHOTOS:PICK_FILES);}catch(Exception e){fail(e);}}
    private void capture(){try{if(attachments.size()>=MAX_ATTACHMENTS)throw new IOException(getString(R.string.attachment_limit));cameraToken=UUID.randomUUID()+".jpg";preferences.edit().putString("cameraToken",cameraToken).apply();Uri uri=CameraOutput.uri(cameraToken);Intent intent=new Intent(MediaStore.ACTION_IMAGE_CAPTURE).putExtra(MediaStore.EXTRA_OUTPUT,uri).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION);intent.setClipData(ClipData.newRawUri("FTA camera",uri));startActivityForResult(intent,CAPTURE);}catch(Exception e){fail(e);}}
    @Override protected void onActivityResult(int request,int result,Intent data){super.onActivityResult(request,result,data);
        if(request==OPEN_RECEIVED){if(result==RESULT_OK&&data!=null&&data.getData()!=null)try{Uri uri=data.getData();String type=getContentResolver().getType(uri);startActivity(new Intent(Intent.ACTION_VIEW).setDataAndType(uri,type==null?"application/octet-stream":type).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION));}catch(Exception e){fail(e);}return;}
        if(request==CAPTURE){String token=cameraToken;cameraToken=null;preferences.edit().remove("cameraToken").apply();if(token==null)return;Uri uri=CameraOutput.uri(token);File captured=new File(getNoBackupFilesDir(),"engine/Outgoing/Camera/"+token);if(result==RESULT_OK&&captured.isFile()&&captured.length()>0)importUris(Collections.singletonList(uri),()->{revokeUriPermission(uri,Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION);new File(NativeBridge.outgoing,"Camera/"+token).delete();});else{revokeUriPermission(uri,Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION);captured.delete();if(result==RESULT_OK)fail(new IOException(getString(R.string.camera_save_failed)));}return;}
        if((request==PICK_PHOTOS||request==PICK_FILES)&&result==RESULT_OK&&data!=null){ArrayList<Uri> uris=new ArrayList<>();if(data.getClipData()!=null)for(int i=0;i<data.getClipData().getItemCount();i++)uris.add(data.getClipData().getItemAt(i).getUri());else if(data.getData()!=null)uris.add(data.getData());importUris(uris,null);}
    }
    // Provider grants can disappear after selection; prepare durable private copies before the Rust source grant.
    private void importUris(List<Uri> uris,Runnable cleanup){if(!ready){deferredUris=new ArrayList<>(uris);deferredCleanup=cleanup;return;}if(importing||sending){if(cleanup!=null)cleanup.run();return;}if(attachments.size()+uris.size()>MAX_ATTACHMENTS){if(cleanup!=null)cleanup.run();fail(new IOException(getString(R.string.attachment_limit)));return;}importing=true;render();WORK.execute(()->{ArrayList<File> copied=new ArrayList<>();File session=new File(NativeBridge.outgoing,UUID.randomUUID().toString());try{
        for(Uri uri:uris){String filename=getString(R.string.files);try(android.database.Cursor cursor=getContentResolver().query(uri,new String[]{OpenableColumns.DISPLAY_NAME},null,null,null)){if(cursor!=null&&cursor.moveToFirst())filename=cursor.getString(0);}
            filename=filename==null?getString(R.string.files):filename.replaceAll("[\\\\/\\p{Cntrl}]","_");if(filename.isEmpty()||filename.equals(".")||filename.equals(".."))filename=getString(R.string.files);
            File slot=new File(session,UUID.randomUUID().toString());if(!slot.mkdirs())throw new IOException(getString(R.string.staging_create_failed));File target=new File(slot,filename);
            try(InputStream input=getContentResolver().openInputStream(uri);FileOutputStream output=new FileOutputStream(target)){if(input==null)throw new IOException(getString(R.string.selected_file_unavailable));byte[] buffer=new byte[64*1024];int n;while((n=input.read(buffer))!=-1){if(destroyed)throw new IOException(getString(R.string.import_interrupted));if(n>0)output.write(buffer,0,n);}output.getFD().sync();}copied.add(target);
        }
        main.post(()->{if(destroyed){deleteTree(session);return;}attachments.addAll(copied);importing=false;persistDraft();render();});
    }catch(Exception e){deleteTree(session);main.post(()->{importing=false;render();});fail(e);}finally{if(cleanup!=null)main.post(cleanup);}});}
    private void restoreAttachments(){try{JSONArray saved=new JSONArray(preferences.getString("attachments","[]"));for(int i=0;i<saved.length();i++){File f=new File(saved.getString(i));if(f.isFile()&&f.getCanonicalPath().equals(f.getAbsolutePath())&&f.getCanonicalPath().startsWith(NativeBridge.outgoing.getCanonicalPath()+File.separator))attachments.add(f);}}catch(Exception ignored){} }
    private void persistDraft(){applyOutcome();JSONArray paths=new JSONArray();for(File file:attachments)paths.put(file.getAbsolutePath());preferences.edit().putString("messageDraft",messageDraft).putString("attachments",paths.toString()).apply();}
    private void clearAttachments(){for(File f:attachments){File parent=f.getParentFile();if(parent!=null&&parent.getParentFile()!=null&&parent.getParentFile().getParentFile()!=null&&parent.getParentFile().getParentFile().equals(NativeBridge.outgoing))deleteTree(parent.getParentFile());}attachments.clear();persistDraft();}
    // A killed import may leave private copies; preserve live drafts/camera results and remove only unreferenced staging.
    private void pruneStaging(){File[] sessions=NativeBridge.outgoing.listFiles();if(sessions==null)return;
        for(File session:sessions){if("Camera".equals(session.getName())){Set<String> keep=new HashSet<>();if(cameraToken!=null)keep.add(cameraToken);if(deferredUris!=null)for(Uri uri:deferredUris)if("app.fta.android.camera".equals(uri.getAuthority()))keep.add(uri.getLastPathSegment());File[] photos=session.listFiles();if(photos!=null)for(File photo:photos)if(!keep.contains(photo.getName()))deleteTree(photo);continue;}
            boolean keep=false;for(File draft:attachments)if(draft.getAbsolutePath().startsWith(session.getAbsolutePath()+File.separator)){keep=true;break;}if(!keep)deleteTree(session);
        }
    }
    private static void deleteTree(File root){try{if(!root.getCanonicalPath().equals(root.getAbsolutePath())){root.delete();return;}}catch(IOException denied){return;}File[] children=root.listFiles();if(children!=null)for(File f:children)deleteTree(f);root.delete();}
    // Persist selection before handing an immutable attempt to the Service; closing this screen no longer owns the transfer.
    private void send(boolean files){if(session==null||!running||sending||importing||selected.isEmpty()||(files?attachments.isEmpty():messageDraft.trim().isEmpty()))return;persistDraft();if(session.send(selected,messageDraft,new ArrayList<>(attachments),files))updateLifecycle();}
    static String messageKind(String body){String trimmed=body.trim();if(trimmed.isEmpty()||trimmed.matches("(?s).*\\s.*"))return "text";try{URI uri=new URI(trimmed);String scheme=uri.getScheme();return ("http".equalsIgnoreCase(scheme)||"https".equalsIgnoreCase(scheme))&&uri.getHost()!=null?"url":"text";}catch(Exception e){return "text";}}
    // Two bounded dim/recover cycles are measured from the stored receipt, never from a view redraw.
    static float completionOpacity(long deadline,long now,boolean animate){
        if(!animate)return 1;long elapsed=now-(deadline-COMPLETION_MS);
        if(elapsed<0||elapsed>=2*BLINK_CYCLE_MS)return 1;
        float phase=(elapsed%BLINK_CYCLE_MS)/(float)BLINK_CYCLE_MS;return 1-0.75f*(1-Math.abs(2*phase-1));
    }
    private void cancel(){if(session!=null)session.cancel();}

    private void renderHistory(){if(historyRows==null)return;
        // Native list rows retain stable accessibility focus when engine polling returns unchanged records.
        String snapshot=history.toString();if(snapshot.equals(renderedHistory))return;renderedHistory=snapshot;historyRows.removeAllViews();
        if(history.length()==0){TextView hint=label(getString(R.string.no_history),16);hint.setTextColor(textSecondary);hint.setPadding(0,dp(24),0,dp(24));historyRows.addView(hint);}
        for(int i=0;i<history.length();i++){JSONObject record=history.optJSONObject(i);if(record==null)continue;
            String summary=("receive".equals(record.optString("direction"))?getString(R.string.received):getString(R.string.sent))+" · "+stateText(record.optString("state"))+"\n"+android.text.format.DateUtils.formatDateTime(this,record.optLong("created_at")*1000,android.text.format.DateUtils.FORMAT_SHOW_DATE|android.text.format.DateUtils.FORMAT_SHOW_TIME);
            View row=settingsRow(title(record),summary,"files".equals(record.optString("kind"))?R.drawable.ic_file:R.drawable.ic_history,()->openReceipt(record.optString("transfer_id")));
            row.setOnLongClickListener(v->{new AlertDialog.Builder(this).setTitle(getString(R.string.delete_record_title)).setMessage(getString(R.string.delete_record_summary)).setNegativeButton(getString(R.string.cancel),null).setPositiveButton(getString(R.string.delete_record),(d,w)->CONTROL.execute(()->{try{NativeBridge.call("inbox.delete",new JSONObject().put("transfer_id",record.optString("transfer_id")));main.post(this::loadHistory);}catch(Exception e){fail(e);}})).show();return true;});
            historyRows.addView(row,new LinearLayout.LayoutParams(-1,-2));historyRows.addView(divider());
        }
    }
    private String title(JSONObject record){if("files".equals(record.optString("kind"))){JSONArray items=record.optJSONArray("items");return items!=null&&items.length()>0?items.optJSONObject(0).optString("name")+(items.length()>1?getResources().getQuantityString(R.plurals.extra_files,items.length()-1,items.length()-1):""):getString(R.string.files);}return "url".equals(record.optString("kind"))?"URL":getString(R.string.text);}
    // Protocol values remain stable; only their presentation is localized, including in-flight history.
    private String kindText(String kind){return "files".equals(kind)?getString(R.string.files):"url".equals(kind)?"URL":getString(R.string.text);}
    private String stateText(String state){switch(state){case "sending":return getString(R.string.state_sending);case "receiving":return getString(R.string.state_receiving);case "verifying":return getString(R.string.state_verifying);case "committing":return getString(R.string.state_saving);case "queued":case "pending":return getString(R.string.state_preparing);case "completed":return getString(R.string.completed);case "partial":return getString(R.string.partially_completed);case "failed":return getString(R.string.failed);case "cancelled":return getString(R.string.cancelled);case "unknown":return getString(R.string.check_result);case "awaiting_registration":return getString(R.string.awaiting_registration);default:return getString(R.string.state_unknown);}}
    private void openReceipt(String id){if(!ready){pendingReceipt=id;return;}WORK.execute(()->{try{JSONObject r=(JSONObject)NativeBridge.call("inbox.get",new JSONObject().put("transfer_id",id));List<GalleryExport.Photo> photos=GalleryExport.candidates(r);main.post(()->showReceipt(r,photos));}catch(Exception e){fail(e);}});}
    private void showReceipt(JSONObject record,List<GalleryExport.Photo> photos){if(isFinishing()||destroyed)return;String bodyText=record.isNull("body")?"":record.optString("body","");String text=stateText(record.optString("state"))+"\n"+bodyText;
        JSONArray items=record.optJSONArray("items");if(items!=null)for(int i=0;i<items.length();i++){JSONObject item=items.optJSONObject(i);if(item!=null)text+="\n"+item.optString("name")+" · "+stateText(item.optString("state"));}
        AlertDialog.Builder b=new AlertDialog.Builder(this).setTitle(title(record)).setMessage(text).setNegativeButton(getString(R.string.close),null);
        if(!bodyText.isEmpty())b.setPositiveButton(getString(R.string.copy),(d,w)->{try{getSystemService(android.content.ClipboardManager.class).setPrimaryClip(ClipData.newPlainText("FTA",bodyText));}catch(RuntimeException error){fail(new IOException(getString(R.string.clipboard_failed)));}});
        if("url".equals(record.optString("kind"))&&"url".equals(messageKind(bodyText)))b.setNeutralButton(getString(R.string.open_link),(d,w)->{try{startActivity(new Intent(Intent.ACTION_VIEW,Uri.parse(bodyText.trim())));}catch(Exception e){fail(e);}});
        else if(items!=null&&items.length()>0&&"receive".equals(record.optString("direction"))&&"completed".equals(record.optString("state")))b.setNeutralButton(getString(R.string.open_file),(d,w)->openReceived(items));
        // A completed incoming photo has a separate explicit gallery action; copying text and opening files remain intact.
        if(!photos.isEmpty())b.setPositiveButton(getString(R.string.save_gallery),(d,w)->chooseGalleryPhotos(photos));b.show();}
    private void chooseGalleryPhotos(List<GalleryExport.Photo> photos){if(savingGallery)return;String[] names=new String[photos.size()];boolean[] selected=new boolean[photos.size()];java.util.Arrays.fill(selected,true);for(int i=0;i<names.length;i++)names[i]=photos.get(i).file.getName();
        AlertDialog dialog=new AlertDialog.Builder(this).setTitle(getString(R.string.save_gallery)).setMultiChoiceItems(names,selected,(d,which,on)->{selected[which]=on;boolean any=false;for(boolean checked:selected)any|=checked;((AlertDialog)d).getButton(AlertDialog.BUTTON_POSITIVE).setEnabled(any);}).setNegativeButton(getString(R.string.cancel),null).setPositiveButton(getString(R.string.save),null).create();
        dialog.setOnShowListener(d->dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(v->{ArrayList<GalleryExport.Photo> chosen=new ArrayList<>();for(int i=0;i<selected.length;i++)if(selected[i])chosen.add(photos.get(i));if(chosen.isEmpty())return;dialog.dismiss();saveGallery(chosen);}));dialog.show();
    }
    private void saveGallery(List<GalleryExport.Photo> photos){if(savingGallery)return;savingGallery=true;java.util.concurrent.atomic.AtomicBoolean cancellation=new java.util.concurrent.atomic.AtomicBoolean();galleryCancellation=cancellation;
        android.widget.ProgressBar progress=new android.widget.ProgressBar(this,null,android.R.attr.progressBarStyleHorizontal);progress.setMax(1000);AlertDialog progressDialog=new AlertDialog.Builder(this).setTitle(getString(R.string.save_gallery)).setMessage(getString(R.string.saving_gallery)).setView(progress).setNegativeButton(getString(R.string.cancel),(d,w)->cancellation.set(true)).create();galleryProgress=progressDialog;progressDialog.setOnCancelListener(d->cancellation.set(true));progressDialog.show();Context app=getApplicationContext();
        WORK.execute(()->{GalleryExport.Result result=GalleryExport.save(app,photos,cancellation,(copied,total)->main.post(()->{if(!destroyed)progress.setProgress(total>0?(int)Math.min(1000,1000.0*copied/total):0);}));
            // Persist only counts/our media URI so rotation or leaving the Activity cannot turn a partial copy into a false success.
            String outcome="";try{JSONObject value=new JSONObject().put("saved",result.saved).put("failed",result.failed).put("total",photos.size()).put("cancelled",result.cancelled);if(result.first!=null)value.put("uri",result.first.toString()).put("mime",result.firstMime);outcome=value.toString();}catch(JSONException ignored){}
            boolean recorded=app.getSharedPreferences(GalleryExport.STATE_PREFS,MODE_PRIVATE).edit().putString(GalleryExport.RESULT_KEY,outcome).commit();String fallback=recorded?null:outcome;
            // If app storage is full, the visible Activity still reports actual counts instead of silently losing the result.
            main.post(()->{savingGallery=false;galleryCancellation=null;pendingGalleryResult=fallback;if(!destroyed){progressDialog.dismiss();galleryProgress=null;consumeGalleryResult();}});
        });
    }
    private void consumeGalleryResult(){if(!visible||destroyed||isFinishing())return;android.content.SharedPreferences store=getSharedPreferences(GalleryExport.STATE_PREFS,MODE_PRIVATE);String raw=store.getString(GalleryExport.RESULT_KEY,pendingGalleryResult);if(raw==null)return;
        try{JSONObject result=new JSONObject(raw);store.edit().remove(GalleryExport.RESULT_KEY).apply();pendingGalleryResult=null;String summary=getString(R.string.gallery_result,result.optInt("saved"),result.optInt("total"),result.optInt("failed"));if(result.optBoolean("cancelled"))summary+="\n"+getString(R.string.gallery_cancelled);
            AlertDialog.Builder dialog=new AlertDialog.Builder(this).setTitle(getString(R.string.gallery_saved_title)).setMessage(summary).setNegativeButton(getString(R.string.close),null);
            if(result.optInt("saved")>0)dialog.setPositiveButton(getString(R.string.view_photo),(d,w)->{try{startActivity(new Intent(Intent.ACTION_VIEW).setDataAndType(Uri.parse(result.getString("uri")),result.getString("mime")).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION));}catch(Exception error){fail(error);}});dialog.show();
        }catch(JSONException invalid){store.edit().remove(GalleryExport.RESULT_KEY).apply();pendingGalleryResult=null;}
    }
    private void openReceived(JSONArray items){ArrayList<File> files=new ArrayList<>();ArrayList<String> names=new ArrayList<>();for(int i=0;i<items.length();i++){JSONObject item=items.optJSONObject(i);if(item!=null){String saved=item.optString("saved_name","");if(!saved.isEmpty()){File file=new File(NativeBridge.received,saved);if(file.isFile()){files.add(file);names.add(saved);}}}}if(files.isEmpty()){fail(new IOException(getString(R.string.received_file_missing)));return;}new AlertDialog.Builder(this).setTitle(getString(R.string.open_file)).setItems(names.toArray(new String[0]),(d,w)->{File file=files.get(w);Uri uri=DocumentsContract.buildDocumentUri("app.fta.android.documents",ReceivedDocuments.documentId(file));try{startActivity(new Intent(Intent.ACTION_VIEW).setDataAndType(uri,ReceivedDocuments.mime(file)).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION));}catch(Exception e){fail(e);}}).show();}
    private void manageDevices(){if(!ready)return;WORK.execute(()->{try{JSONArray devices=(JSONArray)NativeBridge.call("devices.list");main.post(()->{String[] names=new String[devices.length()];for(int i=0;i<names.length;i++){JSONObject p=devices.optJSONObject(i);names[i]=p.optString("display_name")+(p.optBoolean("paired")?getString(R.string.registered_suffix):getString(R.string.discovered_suffix));}new AlertDialog.Builder(this).setTitle(getString(R.string.unregister_title)).setItems(names,(d,w)->{JSONObject p=devices.optJSONObject(w);if(!p.optBoolean("paired"))return;new AlertDialog.Builder(this).setTitle(getString(R.string.unregister_this_device)).setMessage(p.optString("display_name")).setNegativeButton(getString(R.string.cancel),null).setPositiveButton(getString(R.string.unregister),(a,b)->CONTROL.execute(()->{try{NativeBridge.call("devices.unpair",new JSONObject().put("device_id",p.optString("device_id")));}catch(Exception e){fail(e);}})).show();}).setNegativeButton(getString(R.string.close),null).show();});}catch(Exception e){fail(e);}});}
    // Long license text loads off the UI thread; legal content is local and contains no transfer data.
    private void showLegal(String asset){WORK.execute(()->{try(InputStream input=getAssets().open(asset.equals("PrivacyPolicy.txt")&&!"ko".equals(getResources().getConfiguration().getLocales().get(0).getLanguage())?"PrivacyPolicy.en.txt":asset);ByteArrayOutputStream bytes=new ByteArrayOutputStream()){byte[] buffer=new byte[8192];int n;while((n=input.read(buffer))!=-1)bytes.write(buffer,0,n);String text=bytes.toString(java.nio.charset.StandardCharsets.UTF_8.name());main.post(()->{if(destroyed||isFinishing())return;TextView value=label(text,12);value.setTextIsSelectable(true);value.setPadding(dp(16),dp(8),dp(16),dp(8));ScrollView scroll=new ScrollView(this);scroll.addView(value);new AlertDialog.Builder(this).setTitle(asset.equals("PrivacyPolicy.txt")?getString(R.string.privacy_policy):getString(R.string.open_source_notices)).setView(scroll).setPositiveButton(getString(R.string.close),null).show();});}catch(Exception e){fail(e);}});}
    private void fail(Exception error){main.post(()->{if(destroyed||!visible||isFinishing())return;String text=error.getMessage()==null?getString(R.string.operation_failed):error.getMessage();if(text.equals(lastError))return;lastError=text;new AlertDialog.Builder(this).setTitle("FTA").setMessage(text).setPositiveButton(getString(R.string.ok),(d,w)->lastError=null).show();});}
    // Touching outside a text input dismisses the keyboard without intercepting the input's own gestures.
    @Override public boolean dispatchTouchEvent(android.view.MotionEvent event){if(event.getAction()==android.view.MotionEvent.ACTION_DOWN){View focus=getCurrentFocus();if(focus instanceof EditText){Rect bounds=new Rect();focus.getGlobalVisibleRect(bounds);if(!bounds.contains((int)event.getRawX(),(int)event.getRawY())){focus.clearFocus();getSystemService(InputMethodManager.class).hideSoftInputFromWindow(focus.getWindowToken(),0);}}}return super.dispatchTouchEvent(event);}
    private int dp(float x){return Math.round(x*getResources().getDisplayMetrics().density);}
    private TextView label(String text,int size){TextView t=new TextView(this);t.setText(text);t.setTextColor(textPrimary);t.setTextSize(size);return t;}
    // An outlined native Button remains visible as an action rather than blending into a list label.
    private Button button(String text,Runnable action){Button b=new Button(this);b.setBackground(actionSurface(surface));b.setTextColor(textPrimary);b.setPadding(dp(16),dp(8),dp(16),dp(8));b.setText(text);b.setAllCaps(false);b.setMinHeight(dp(48));b.setOnClickListener(v->action.run());return b;}
    // Balance the text area against the left drawable so only the labels move to the button center; icons remain at their existing start padding.
    private void setButtonIcon(Button button,int resource){Drawable icon=getDrawable(resource);icon.setTint(accent);button.setCompoundDrawablesWithIntrinsicBounds(icon,null,null,null);button.setCompoundDrawablePadding(dp(8));button.setPadding(dp(16),0,dp(16)+icon.getIntrinsicWidth()+dp(8),0);button.setGravity(Gravity.START|Gravity.CENTER_VERTICAL);button.setTextAlignment(View.TEXT_ALIGNMENT_CENTER);}
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
