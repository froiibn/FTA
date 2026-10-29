package app.fta.android;

import android.content.*;
import android.database.*;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import java.io.*;

// The camera receives a scoped URI for one random JPEG; arbitrary native paths are never accepted.
public final class CameraOutput extends ContentProvider {
    public boolean onCreate(){return true;}
    static Uri uri(String token){return new Uri.Builder().scheme("content").authority("app.fta.android.camera").appendPath(token).build();}
    private File file(Uri uri)throws FileNotFoundException{
        String token=uri.getLastPathSegment();if(token==null||!token.matches("[0-9a-f-]{36}\\.jpg")||uri.getPathSegments().size()!=1)throw new FileNotFoundException();
        File root=new File(getContext().getNoBackupFilesDir(),"engine/Outgoing/Camera");
        if(!root.isDirectory()&&!root.mkdirs())throw new FileNotFoundException();return new File(root,token);
    }
    public String getType(Uri uri){return "image/jpeg";}
    public ParcelFileDescriptor openFile(Uri uri,String mode)throws FileNotFoundException{return ParcelFileDescriptor.open(file(uri),ParcelFileDescriptor.parseMode(mode));}
    public Cursor query(Uri u,String[] projection,String selection,String[] args,String sort){MatrixCursor c=new MatrixCursor(new String[]{OpenableColumns.DISPLAY_NAME,OpenableColumns.SIZE});try{File f=file(u);c.addRow(new Object[]{f.getName(),f.length()});}catch(Exception ignored){}return c;}
    public Uri insert(Uri u,ContentValues v){throw new UnsupportedOperationException();}public int update(Uri u,ContentValues v,String s,String[] a){throw new UnsupportedOperationException();}public int delete(Uri u,String s,String[] a){throw new UnsupportedOperationException();}
}
