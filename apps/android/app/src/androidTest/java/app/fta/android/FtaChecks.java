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
    @Override public void onStart(){Bundle result=new Bundle();try{
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
