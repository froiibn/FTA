package app.fta.android;

import android.content.Context;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.AtomicFile;
import org.json.JSONObject;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import java.util.Arrays;
import javax.crypto.*;
import javax.crypto.spec.GCMParameterSpec;

// A synchronous bridge runs only on worker threads; private keys never enter JSON controls or logs.
final class NativeBridge {
    private static final String WRAPPING_ALIAS = "FTA.identity.wrapper.v1";
    // JNI workers retain only the process Application, never an Activity or its views.
    private static android.app.Application context;
    static { System.loadLibrary("fta_android"); }
    private static native byte[] nativeCall(byte[] request);
    private static native int nativeConfigure(String root, String state, String received);
    static File state, outgoing, received;

    static void initialize(Context app) throws Exception {
        context = (android.app.Application) app.getApplicationContext();
        state = new File(context.getNoBackupFilesDir(),"engine");
        outgoing = new File(state,"Outgoing");
        received = new File(context.getFilesDir(),"Received");
        for(File f:new File[]{state,outgoing,received}) if(!f.isDirectory() && !f.mkdirs()) throw new IOException(context.getString(R.string.storage_create_failed));
        // Canonicalize OS aliases once so restored selection grants keep the same physical spelling.
        state=state.getCanonicalFile();outgoing=outgoing.getCanonicalFile();received=received.getCanonicalFile();
        if(nativeConfigure(context.getDataDir().getCanonicalPath(),state.getCanonicalPath(),received.getCanonicalPath()) != 0) throw new IOException(context.getString(R.string.storage_init_failed));
        call("host.prepare",paths());
    }
    static JSONObject paths() throws Exception { return new JSONObject().put("state_dir",state.getCanonicalPath()).put("receive_dir",received.getCanonicalPath()); }
    static Object call(String method,JSONObject params) throws Exception {
        byte[] answer = nativeCall(new JSONObject().put("method",method).put("params",params).toString().getBytes(StandardCharsets.UTF_8));
        if(answer==null) throw new IOException(context.getString(R.string.engine_request_failed));
        JSONObject envelope=new JSONObject(new String(answer,StandardCharsets.UTF_8));
        // Present localized guidance while retaining the stable error code; never surface raw engine paths or payloads.
        if(envelope.has("error")){JSONObject e=envelope.getJSONObject("error");throw new IOException(e.optString("code")+": "+context.getString(R.string.engine_error,e.optString("code")));}
        return envelope.opt("result");
    }
    // Stored transfer errors are technical codes, not translated UI or user content. Keep them out of localized sentences.
    static String transferError(JSONObject transfer){String code=transfer.optString("error","");if(!code.matches("[A-Z][A-Z_0-9]{1,63}"))code="RESULT_UNCONFIRMED";return context.getString(R.string.engine_error,code);}
    static Object call(String method) throws Exception { return call(method,new JSONObject()); }

    private static SecretKey wrappingKey(boolean create) throws Exception {
        KeyStore store=KeyStore.getInstance("AndroidKeyStore");store.load(null);
        if(store.containsAlias(WRAPPING_ALIAS)) return (SecretKey)store.getKey(WRAPPING_ALIAS,null);
        // A missing wrapping key for an existing identity is an error, never a silent identity reset.
        if(!create) throw new IOException(context.getString(R.string.keystore_unavailable));
        KeyGenerator generator=KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES,"AndroidKeyStore");
        generator.init(new KeyGenParameterSpec.Builder(WRAPPING_ALIAS,KeyProperties.PURPOSE_ENCRYPT|KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).setKeySize(256).build());
        return generator.generateKey();
    }
    private static File secretFile(String account) throws IOException {
        if(!account.matches("[0-9a-f]{64}")) throw new IOException(context.getString(R.string.invalid_key));
        File folder=new File(context.getNoBackupFilesDir(),"vault");
        if(!folder.isDirectory()&&!folder.mkdirs())throw new IOException(context.getString(R.string.keystore_create_failed));
        return new File(folder,account+".gcm");
    }
    // Called by JNI on attached Rust workers. AES-GCM authenticates the device/account binding at rest.
    private static synchronized void storeSecret(String account,byte[] secret) throws Exception {
        try {
            Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");cipher.init(Cipher.ENCRYPT_MODE,wrappingKey(true));
            cipher.updateAAD(account.getBytes(StandardCharsets.US_ASCII));byte[] encrypted=cipher.doFinal(secret);
            AtomicFile file=new AtomicFile(secretFile(account));FileOutputStream stream=file.startWrite();
            try {stream.write(cipher.getIV().length);stream.write(cipher.getIV());stream.write(encrypted);file.finishWrite(stream);}
            catch(Exception error){file.failWrite(stream);throw error;}
        } finally { Arrays.fill(secret,(byte)0); }
    }
    private static synchronized byte[] loadSecret(String account) throws Exception {
        byte[] blob=new AtomicFile(secretFile(account)).readFully();
        if(blob.length<30 || (blob[0]&255)!=12)throw new IOException(context.getString(R.string.keystore_corrupt));
        Cipher cipher=Cipher.getInstance("AES/GCM/NoPadding");cipher.init(Cipher.DECRYPT_MODE,wrappingKey(false),new GCMParameterSpec(128,blob,1,12));
        cipher.updateAAD(account.getBytes(StandardCharsets.US_ASCII));return cipher.doFinal(blob,13,blob.length-13);
    }
}
