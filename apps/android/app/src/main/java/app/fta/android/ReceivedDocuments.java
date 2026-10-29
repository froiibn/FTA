package app.fta.android;

import android.database.*;
import android.os.*;
import android.provider.DocumentsContract;
import android.provider.DocumentsProvider;
import android.webkit.MimeTypeMap;
import java.io.*;
import java.util.*;

// Expose only committed receive files through Android's Files UI, never the engine/vault or hidden partials.
public final class ReceivedDocuments extends DocumentsProvider {
    private static final String ROOT="received";
    private static final String[] ROOT_COLUMNS={DocumentsContract.Root.COLUMN_ROOT_ID,DocumentsContract.Root.COLUMN_DOCUMENT_ID,DocumentsContract.Root.COLUMN_TITLE,DocumentsContract.Root.COLUMN_FLAGS,DocumentsContract.Root.COLUMN_MIME_TYPES};
    private static final String[] DOC_COLUMNS={DocumentsContract.Document.COLUMN_DOCUMENT_ID,DocumentsContract.Document.COLUMN_DISPLAY_NAME,DocumentsContract.Document.COLUMN_MIME_TYPE,DocumentsContract.Document.COLUMN_FLAGS,DocumentsContract.Document.COLUMN_SIZE,DocumentsContract.Document.COLUMN_LAST_MODIFIED};
    // Files can discover this provider before the first Activity launch; its public root must already be a directory.
    public boolean onCreate(){File folder=root();return folder.isDirectory()||folder.mkdirs();}
    private File root(){try{return new File(getContext().getFilesDir(),"Received").getCanonicalFile();}catch(IOException e){throw new IllegalStateException("수신 폴더를 열 수 없습니다",e);}}
    private File checked(String id)throws FileNotFoundException{
        try{
            if(ROOT.equals(id))return root().getCanonicalFile();
            if(!id.startsWith(ROOT+":"))throw new IOException();
            String name=new String(android.util.Base64.decode(id.substring(ROOT.length()+1),android.util.Base64.URL_SAFE|android.util.Base64.NO_WRAP),java.nio.charset.StandardCharsets.UTF_8);
            // Final dotfiles are valid payloads; directories/nested paths keep private partials out of this flat provider.
            if(name.isEmpty()||name.equals(".")||name.equals("..")||name.contains("/")||name.contains("\\"))throw new IOException();
            File candidate=new File(root(),name);
            if(!candidate.getCanonicalFile().getParentFile().equals(root().getCanonicalFile())||!candidate.isFile())throw new IOException();
            return candidate;
        }catch(Exception e){throw new FileNotFoundException("수신 파일을 찾을 수 없습니다");}
    }
    static String documentId(File file){return ROOT+":"+android.util.Base64.encodeToString(file.getName().getBytes(java.nio.charset.StandardCharsets.UTF_8),android.util.Base64.URL_SAFE|android.util.Base64.NO_WRAP);}
    private void row(MatrixCursor c,File f){MatrixCursor.RowBuilder r=c.newRow();
        r.add(DocumentsContract.Document.COLUMN_DOCUMENT_ID,f.equals(root())?ROOT:documentId(f));
        r.add(DocumentsContract.Document.COLUMN_DISPLAY_NAME,f.equals(root())?"FTA 받은 파일":f.getName());
        r.add(DocumentsContract.Document.COLUMN_MIME_TYPE,f.isDirectory()?DocumentsContract.Document.MIME_TYPE_DIR:mime(f));
        r.add(DocumentsContract.Document.COLUMN_FLAGS,f.isFile()?DocumentsContract.Document.FLAG_SUPPORTS_DELETE:0);r.add(DocumentsContract.Document.COLUMN_SIZE,f.isFile()?f.length():0);r.add(DocumentsContract.Document.COLUMN_LAST_MODIFIED,f.lastModified());}
    static String mime(File f){String n=f.getName(),ext=n.contains(".")?n.substring(n.lastIndexOf('.')+1).toLowerCase(Locale.ROOT):"";String m=MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext);return m==null?"application/octet-stream":m;}
    public Cursor queryRoots(String[] projection){MatrixCursor c=new MatrixCursor(projection==null?ROOT_COLUMNS:projection);MatrixCursor.RowBuilder r=c.newRow();r.add(DocumentsContract.Root.COLUMN_ROOT_ID,ROOT);r.add(DocumentsContract.Root.COLUMN_DOCUMENT_ID,ROOT);r.add(DocumentsContract.Root.COLUMN_TITLE,"FTA");r.add(DocumentsContract.Root.COLUMN_FLAGS,DocumentsContract.Root.FLAG_LOCAL_ONLY);r.add(DocumentsContract.Root.COLUMN_MIME_TYPES,"*/*");return c;}
    public Cursor queryDocument(String id,String[] projection)throws FileNotFoundException{MatrixCursor c=new MatrixCursor(projection==null?DOC_COLUMNS:projection);row(c,checked(id));return c;}
    public Cursor queryChildDocuments(String parent,String[] projection,String sort)throws FileNotFoundException{File dir=checked(parent);if(!dir.equals(root()))throw new FileNotFoundException();MatrixCursor c=new MatrixCursor(projection==null?DOC_COLUMNS:projection);File[] files=dir.listFiles();if(files!=null)for(File f:files)if(f.isFile())try{row(c,checked(documentId(f)));}catch(FileNotFoundException ignored){}return c;}
    public ParcelFileDescriptor openDocument(String id,String mode,CancellationSignal signal)throws FileNotFoundException{if(!"r".equals(mode))throw new FileNotFoundException("읽기만 허용됩니다");if(signal!=null)signal.throwIfCanceled();File f=checked(id);if(!f.isFile())throw new FileNotFoundException();return ParcelFileDescriptor.open(f,ParcelFileDescriptor.MODE_READ_ONLY);}
    // Explicit Files-app deletion affects only one final received file, never the root or private staging.
    @Override public void deleteDocument(String id)throws FileNotFoundException{File f=checked(id);if(!f.isFile()||!f.delete())throw new FileNotFoundException("수신 파일을 삭제하지 못했습니다");}
    public boolean isChildDocument(String parent,String child){try{return ROOT.equals(parent)&&!ROOT.equals(child)&&checked(child).isFile();}catch(Exception e){return false;}}
}
