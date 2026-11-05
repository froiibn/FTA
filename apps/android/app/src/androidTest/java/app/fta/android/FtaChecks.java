package app.fta.android;

import android.app.*;
import android.content.*;
import android.database.Cursor;
import android.os.*;
import android.provider.DocumentsContract;
import org.json.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

// Framework-only instrumentation exercises the actual JNI/Keystore host, not a mocked Android engine.
public final class FtaChecks extends Instrumentation {
    private int checks;
    private Bundle arguments;
    @Override public void onCreate(Bundle args){super.onCreate(args);arguments=args;start();}
    private void check(boolean condition,String name){if(!condition)throw new AssertionError(name);checks++;}
    private String read(File file)throws Exception{try(InputStream input=new FileInputStream(file)){return new String(input.readAllBytes(),StandardCharsets.UTF_8);}}
    @Override public void onStart(){if("true".equals(arguments.getString("gallery"))){new GalleryChecks(this).run();return;}if("true".equals(arguments.getString("background"))){new BackgroundChecks(this).run();return;}Bundle result=new Bundle();try{
        // Native channel policy checks avoid mutating any user-managed channel or posting synthetic UI receipts.
        android.app.NotificationChannel normal=new android.app.NotificationChannel("receipt-test","test",android.app.NotificationManager.IMPORTANCE_DEFAULT);
        check(!ReceiptAlerts.preserveChannel(normal),"unchanged default channel may adopt heads-up default");
        normal.setSound(null,null);check(ReceiptAlerts.preserveChannel(normal),"explicitly silent legacy channel stays silent");
        android.app.NotificationChannel blocked=new android.app.NotificationChannel("receipt-blocked-test","test",android.app.NotificationManager.IMPORTANCE_NONE);
        check(ReceiptAlerts.preserveChannel(blocked),"blocked legacy channel is never bypassed");
        NativeBridge.initialize(getTargetContext());
        JSONObject identity=new JSONObject(read(new File(NativeBridge.state,"identity.json")));
        check(!identity.has("key"),"private identity must never be plaintext JSON");
        String account=identity.getString("keychain_account");check(account.matches("[0-9a-f]{64}"),"Keystore identity reference");
        check(new File(getTargetContext().getNoBackupFilesDir(),"vault/"+account+".gcm").length()>29,"authenticated encrypted vault exists");
        JSONObject settings=(JSONObject)NativeBridge.call("settings.get");String before=settings.getString("device_id");
        NativeBridge.call("host.stop");NativeBridge.call("host.prepare",NativeBridge.paths());
        check(before.equals(((JSONObject)NativeBridge.call("settings.get")).getString("device_id")),"identity remains stable after native restart");
        check(account.equals(new JSONObject(read(new File(NativeBridge.state,"identity.json"))).getString("keychain_account")),"wrapped key account remains stable");
        check("url".equals(MainActivity.messageKind(" https://example.com/path?q=1 ")),"single HTTP URL");
        check("text".equals(MainActivity.messageKind("https://example.com\nsecond line")),"multiline body stays text");
        check("text".equals(MainActivity.messageKind("file:///private/secret")),"non-HTTP scheme stays text");
        JSONObject transfer=new JSONObject().put("kind","files").put("items",new JSONArray().put(new JSONObject().put("size","100").put("bytes_transferred","100")).put(new JSONObject().put("size","300").put("bytes_transferred","100")));
        check(Math.abs(MainActivity.fileFraction(transfer)-0.5)<0.00001,"combined real byte fraction");
        check(Double.isNaN(MainActivity.fileFraction(new JSONObject().put("kind","text"))),"no fabricated text percentage");
        check(Double.isNaN(MainActivity.fileFraction(new JSONObject().put("kind","files").put("items",new JSONArray()))),"unknown file size stays unknown");
        for(long offset:new long[]{0,600,1200,1500,2000})check(Math.abs(MainActivity.completionOpacity(1500,offset,true)-1)<0.00001,"receipt blink restores brightness");
        for(long offset:new long[]{300,900})check(Math.abs(MainActivity.completionOpacity(1500,offset,true)-0.25)<0.00001,"exactly two receipt dim phases");
        check(MainActivity.completionOpacity(1500,300,false)==1,"disabled system animations keep steady completion");
        check(MainActivity.completionOpacity(1500,-1,true)==1,"no blink before completion");
        // Pure display decisions catch the real unpaired discovery regression without injecting devices into UI/engine.
        check(MainActivity.availableTarget(new JSONObject().put("paired",false).put("online",false)),"reachable unpaired target is displayed before approval");
        check(MainActivity.availableTarget(new JSONObject().put("paired",true).put("online",true)),"registered online target remains displayed");
        check(!MainActivity.availableTarget(new JSONObject().put("paired",true).put("online",false)),"registered offline target stays hidden");
        check(!MainActivity.availableTarget(null),"missing device is not selectable");
        File visible=File.createTempFile("FTA-check-",".txt",NativeBridge.received);try(FileOutputStream out=new FileOutputStream(visible)){out.write("receive test".getBytes(StandardCharsets.UTF_8));}
        ReceivedDocuments provider=new ReceivedDocuments();provider.attachInfo(getTargetContext(),getTargetContext().getPackageManager().resolveContentProvider("app.fta.android.documents",0));
        try(Cursor roots=provider.queryRoots(null)){check(roots.moveToFirst(),"Files root is available");}
        try(Cursor docs=provider.queryChildDocuments("received",null,(String)null)){boolean found=false;while(docs.moveToNext())if(visible.getName().equals(docs.getString(docs.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DISPLAY_NAME))))found=true;check(found,"final committed file is visible");}
        String privateId="received:"+android.util.Base64.encodeToString(".fta-partials".getBytes(StandardCharsets.UTF_8),android.util.Base64.URL_SAFE|android.util.Base64.NO_WRAP);
        try{provider.queryDocument(privateId,null);throw new AssertionError("hidden staging must fail");}catch(FileNotFoundException expected){checks++;}
        String escape="received:"+android.util.Base64.encodeToString("../no_backup/engine/identity.json".getBytes(StandardCharsets.UTF_8),android.util.Base64.URL_SAFE|android.util.Base64.NO_WRAP);
        try{provider.queryDocument(escape,null);throw new AssertionError("outside path must fail");}catch(FileNotFoundException expected){checks++;}
        File dotfile=new File(NativeBridge.received,".FTA-check-"+UUID.randomUUID());try(FileOutputStream dot=new FileOutputStream(dotfile)){dot.write(1);}try(Cursor dot=provider.queryDocument(ReceivedDocuments.documentId(dotfile),null)){check(dot.moveToFirst(),"valid final dotfiles stay accessible");}provider.deleteDocument(ReceivedDocuments.documentId(dotfile));
        provider.deleteDocument(ReceivedDocuments.documentId(visible));check(!visible.exists(),"Files-app deletion removes only a final file");
        try{provider.deleteDocument("received");throw new AssertionError("root deletion must fail");}catch(FileNotFoundException expected){checks++;}
        JSONObject cursor=(JSONObject)NativeBridge.call("events.cursor",new JSONObject().put("consumer","android-ui"));check(cursor.has("sequence"),"notification consumer contract");
        if("true".equals(arguments.getString("ui")))uiChecks();
        if("true".equals(arguments.getString("settingsUi")))settingsUiChecks();
        if("true".equals(arguments.getString("integration")))integration(result);
        result.putString("stream","FTA Android checks passed: "+checks+"; actual JNI/Keystore/identity/File provider/URL/progress verified\n");finish(Activity.RESULT_OK,result);
    }catch(Throwable failure){result.putString("stream","FTA Android checks failed: "+failure.getClass().getSimpleName()+": "+failure.getMessage()+"\n");finish(Activity.RESULT_CANCELED,result);}}

    private android.widget.EditText input(android.view.View view){if(view instanceof android.widget.EditText)return (android.widget.EditText)view;if(view instanceof android.view.ViewGroup){android.view.ViewGroup group=(android.view.ViewGroup)view;for(int i=0;i<group.getChildCount();i++){android.widget.EditText found=input(group.getChildAt(i));if(found!=null)return found;}}return null;}
    // Measure the real native input after six lines, then inject a blank-area touch through Android's input stack.
    private void uiChecks()throws Exception{
        MainActivity activity=(MainActivity)startActivitySync(new Intent(getTargetContext(),MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        waitForIdleSync();android.widget.EditText editor=input(activity.getWindow().getDecorView());check(editor!=null,"native message editor exists");String old=editor.getText().toString();int[] one={0};
        runOnMainSync(()->{editor.setText("one");editor.requestFocus();});waitForIdleSync();one[0]=editor.getHeight();
        runOnMainSync(()->{editor.setText("one\ntwo\nthree\nfour\nfive\nsix");editor.setSelection(editor.length());getTargetContext().getSystemService(android.view.inputmethod.InputMethodManager.class).showSoftInput(editor,android.view.inputmethod.InputMethodManager.SHOW_IMPLICIT);});
        Thread.sleep(700);waitForIdleSync();
        check(editor.getHeight()>one[0],"native editor grows from one line");
        check(editor.getHeight()<=editor.getLayout().getLineTop(4)+editor.getCompoundPaddingTop()+editor.getCompoundPaddingBottom()+2,"native editor stops at four actual laid-out lines: height="+editor.getHeight()+", fourLines="+editor.getLayout().getLineTop(4)+", padding="+(editor.getCompoundPaddingTop()+editor.getCompoundPaddingBottom())+", max="+editor.getMaxLines());
        check(editor.canScrollVertically(-1)||editor.canScrollVertically(1),"extra lines scroll within native editor");
        long now=SystemClock.uptimeMillis();android.view.MotionEvent down=android.view.MotionEvent.obtain(now,now,android.view.MotionEvent.ACTION_DOWN,35,150,0),up=android.view.MotionEvent.obtain(now,now+40,android.view.MotionEvent.ACTION_UP,35,150,0);
        getUiAutomation().injectInputEvent(down,true);getUiAutomation().injectInputEvent(up,true);down.recycle();up.recycle();Thread.sleep(500);waitForIdleSync();
        check(!editor.hasFocus(),"blank-area touch dismisses message focus");
        if(Build.VERSION.SDK_INT>=30)check(!activity.getWindow().getDecorView().getRootWindowInsets().isVisible(android.view.WindowInsets.Type.ime()),"blank-area touch hides keyboard");
        runOnMainSync(()->{editor.setText(old);activity.finish();});
    }

    // Text-only labels may have no content description; icon actions expose their name there instead.
    private android.widget.TextView text(android.view.View view,String value){if(view instanceof android.widget.TextView&&(value.contentEquals(((android.widget.TextView)view).getText())||(view.getContentDescription()!=null&&value.contentEquals(view.getContentDescription()))))return (android.widget.TextView)view;if(view instanceof android.view.ViewGroup){android.view.ViewGroup group=(android.view.ViewGroup)view;for(int i=0;i<group.getChildCount();i++){android.widget.TextView found=text(group.getChildAt(i),value);if(found!=null)return found;}}return null;}
    private android.view.View nameDialog(MainActivity activity){for(android.view.View root:android.view.inspector.WindowInspector.getGlobalWindowViews())if(root!=activity.getWindow().getDecorView()&&input(root)!=null)return root;throw new AssertionError("native name dialog exists");}
    // Exercise real framework dialogs/Editable and validators without asynchronous shell/IME key injection.
    private void settingsUiChecks()throws Exception{
        MainActivity activity=(MainActivity)startActivitySync(new Intent(getTargetContext(),MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));waitForIdleSync();
        android.content.SharedPreferences prefs=getTargetContext().getSharedPreferences("fta",Context.MODE_PRIVATE);String previous=prefs.getString("deviceName",Build.MODEL),restore=arguments.getString("restoreName",previous);String valid="FTA Android 설정 검사";
        try{
            runOnMainSync(()->{text(activity.getWindow().getDecorView(),getTargetContext().getString(R.string.settings)).performClick();((android.view.View)text(activity.getWindow().getDecorView(),getTargetContext().getString(R.string.device_name)).getParent().getParent()).performClick();});waitForIdleSync();
            android.view.View first=nameDialog(activity);check(previous.contentEquals(input(first).getText()),"name dialog shows current value");runOnMainSync(()->text(first,getTargetContext().getString(R.string.cancel)).performClick());waitForIdleSync();check(previous.equals(prefs.getString("deviceName",Build.MODEL)),"cancel preserves name");
            runOnMainSync(()->((android.view.View)text(activity.getWindow().getDecorView(),getTargetContext().getString(R.string.device_name)).getParent().getParent()).performClick());waitForIdleSync();android.view.View dialog=nameDialog(activity);android.widget.EditText editor=input(dialog);
            runOnMainSync(()->{editor.setText("");text(dialog,getTargetContext().getString(R.string.save)).performClick();});check(editor.getError()!=null&&previous.equals(prefs.getString("deviceName",Build.MODEL)),"empty name rejected without saving");
            runOnMainSync(()->{editor.setText(new String(new char[43]).replace('\0','가'));text(dialog,getTargetContext().getString(R.string.save)).performClick();});check(editor.getError()!=null&&previous.equals(prefs.getString("deviceName",Build.MODEL)),"UTF-8 name over 128 bytes rejected");
            runOnMainSync(()->{editor.setText(valid);text(dialog,getTargetContext().getString(R.string.save)).performClick();});waitForIdleSync();check(valid.equals(prefs.getString("deviceName",Build.MODEL)),"native dialog stores exact Unicode name");check(text(activity.getWindow().getDecorView(),valid)!=null,"settings row refreshes current value");
        }finally{
            runOnMainSync(()->{for(android.view.View root:android.view.inspector.WindowInspector.getGlobalWindowViews())if(root!=activity.getWindow().getDecorView()&&text(root,getTargetContext().getString(R.string.cancel))!=null)text(root,getTargetContext().getString(R.string.cancel)).performClick();});
            runOnMainSync(()->((android.view.View)text(activity.getWindow().getDecorView(),getTargetContext().getString(R.string.device_name)).getParent().getParent()).performClick());waitForIdleSync();android.view.View dialog=nameDialog(activity);runOnMainSync(()->{input(dialog).setText(restore);text(dialog,getTargetContext().getString(R.string.save)).performClick();});waitForIdleSync();check(restore.equals(prefs.getString("deviceName",Build.MODEL)),"original name restored through dialog");
            // Instrumentation termination can precede apply() disk flushing; wait for the same restored value before exiting the test process.
            if(!prefs.edit().putString("deviceName",restore).commit())throw new AssertionError("restored name persisted");runOnMainSync(()->{text(activity.getWindow().getDecorView(),getTargetContext().getString(R.string.send)).performClick();activity.finish();});
        }
    }

    // Test-only listeners are loopback-bound and never enter the product UI's discovered-device list.
    private void integration(Bundle result)throws Exception {
        Set<String> previousIds=new HashSet<>();JSONArray previous=(JSONArray)NativeBridge.call("inbox.list");for(int i=0;i<previous.length();i++)previousIds.add(previous.getJSONObject(i).getString("transfer_id"));
        JSONObject params=NativeBridge.paths().put("display_name","FTA Android verification").put("listen_addr","127.0.0.1:0").put("allowed_networks",new JSONArray().put("127.0.0.0/8").put("10.0.2.0/24"));
        JSONObject advert=(JSONObject)NativeBridge.call("host.start",params);
        File signal=new File(getTargetContext().getFilesDir(),"integration-endpoints.json");try(FileOutputStream stream=new FileOutputStream(signal)){stream.write(advert.toString().getBytes(StandardCharsets.UTF_8));}
        long deadline=SystemClock.elapsedRealtime()+90000;boolean committed=false;String receivedPeer=null;
        while(SystemClock.elapsedRealtime()<deadline){
            JSONArray requests=(JSONArray)NativeBridge.call("registration.requests");
            for(int i=0;i<requests.length();i++){JSONObject r=requests.getJSONObject(i);if("FTA Mac verification".equals(r.optString("display_name"))){check(!r.has("body"),"approval metadata contains no body");NativeBridge.call("registration.accept",new JSONObject().put("session_id",r.getString("session_id")));}}
            JSONArray records=(JSONArray)NativeBridge.call("inbox.list");
            for(int i=0;i<records.length();i++){JSONObject record=records.getJSONObject(i);if(previousIds.contains(record.optString("transfer_id")))continue;if("receive".equals(record.optString("direction"))&&"completed".equals(record.optString("state"))&&"files".equals(record.optString("kind"))){JSONArray items=record.getJSONArray("items");for(int j=0;j<items.length();j++){JSONObject item=items.getJSONObject(j);if("android-transport-check.txt".equals(item.optString("name"))){File saved=new File(NativeBridge.received,item.getString("saved_name"));check("actual TLS to Android ✓".equals(read(saved)),"Android final file content");committed=true;receivedPeer=record.optString("peer_id");}}}}
            if(committed)break;Thread.sleep(100);
        }
        check(committed,"actual TLS receiver final commit");
        JSONArray peers=(JSONArray)NativeBridge.call("devices.list");String mac=null;
        for(int i=0;i<peers.length();i++)if(peers.getJSONObject(i).optString("device_id").equals(receivedPeer))mac=receivedPeer;
        check(mac!=null,"receiver approval persists authenticated peer");
        try{NativeBridge.call("transfers.send_discovered",new JSONObject().put("to",mac).put("kind","files").put("paths",new JSONArray().put(new File(NativeBridge.state,"identity.json").getCanonicalPath())));throw new AssertionError("private state grant must fail");}catch(IOException denied){check(denied.getMessage().startsWith("PATH_DENIED"),"JNI refuses state outside selection grant");}
        File folder=new File(NativeBridge.outgoing,"android-test/"+UUID.randomUUID());if(!folder.isDirectory()&&!folder.mkdirs())throw new IOException("source staging failed");
        File reply=new File(folder,"android-reply.txt");try(FileOutputStream stream=new FileOutputStream(reply)){stream.write("actual Android JNI stream ✓".getBytes(StandardCharsets.UTF_8));}
        JSONObject sent=(JSONObject)NativeBridge.call("transfers.send_discovered",new JSONObject().put("to",mac).put("kind","files").put("paths",new JSONArray().put(reply.getCanonicalPath())));
        check("completed".equals(sent.optString("state")),"Android source grant, stream and Mac saved receipt");
        boolean text=false,url=false;long bodiesDeadline=SystemClock.elapsedRealtime()+30000;
        while(SystemClock.elapsedRealtime()<bodiesDeadline&&(!text||!url)){
            JSONArray rows=(JSONArray)NativeBridge.call("inbox.list");for(int i=0;i<rows.length();i++){JSONObject row=rows.getJSONObject(i);if(!mac.equals(row.optString("peer_id"))||!"receive".equals(row.optString("direction"))||!"completed".equals(row.optString("state")))continue;
                if("text".equals(row.optString("kind"))||"url".equals(row.optString("kind"))){JSONObject detailed=(JSONObject)NativeBridge.call("inbox.get",new JSONObject().put("transfer_id",row.getString("transfer_id")));text|="Android text ✓\n둘째 줄".equals(detailed.optString("body"));url|="https://example.com/fta?from=android-check".equals(detailed.optString("body"));}}
            if(!text||!url)Thread.sleep(100);
        }
        check(text&&url,"actual Unicode text and URL are committed without changing their bodies");
        NativeBridge.call("devices.unpair",new JSONObject().put("device_id",mac));
        NativeBridge.call("host.stop");signal.delete();
    }
}
