package app.fta.android;

import android.content.*;
import android.database.Cursor;
import android.graphics.*;
import android.net.Uri;
import android.os.Environment;
import android.provider.MediaStore;
import org.json.*;
import java.io.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicBoolean;

// Gallery copies are a separate, explicit operation after final receiving; they never alter a transfer result.
final class GalleryExport {
    static final String STATE_PREFS="gallery_pending_v1",RESULT_KEY="result";
    private static final String PENDING="uris",ALBUM_DIRECTORY=Environment.DIRECTORY_PICTURES+"/FTA/";
    private static final int MAX_DECODE_PIXELS=1_048_576,BUFFER_BYTES=64*1024;
    static final class Photo {final File file;final String mime;Photo(File f,String m){file=f;mime=m;}}
    static final class Result {int saved,failed;boolean cancelled;Uri first;String firstMime;}
    interface Progress {void bytes(long copied,long total);}
    // Reuse only final item names from a completed incoming record, never arbitrary caller paths or staging files.
    static List<Photo> candidates(JSONObject record)throws IOException {
        ArrayList<Photo> photos=new ArrayList<>();
        if(!"receive".equals(record.optString("direction"))||!"completed".equals(record.optString("state"))||!"files".equals(record.optString("kind")))return photos;
        JSONArray items=record.optJSONArray("items");if(items==null)return photos;
        HashSet<String> unique=new HashSet<>();
        for(int i=0;i<items.length();i++){JSONObject item=items.optJSONObject(i);if(item==null||!"completed".equals(item.optString("state")))continue;
            String name=item.optString("saved_name","");if(!unique.add(name))continue;
            Photo photo=verified(NativeBridge.received,name);if(photo!=null)photos.add(photo);
        }return photos;
    }
    static Photo verified(File root,String name)throws IOException {
        if(name.isEmpty()||name.equals(".")||name.equals("..")||name.indexOf('/')>=0||name.indexOf('\\')>=0)return null;
        File file=new File(root,name);if(!file.isFile()||java.nio.file.Files.isSymbolicLink(file.toPath())||!file.getCanonicalFile().getParentFile().equals(root.getCanonicalFile())||file.length()==0)return null;
        String extension=name.contains(".")?name.substring(name.lastIndexOf('.')+1).toLowerCase(Locale.ROOT):"";
        final String[] mime={null};
        try {
            // Decode a bounded bitmap with partial images rejected; extension alone never grants gallery publication.
            Bitmap image=ImageDecoder.decodeBitmap(ImageDecoder.createSource(file),(decoder,info,source)->{
                mime[0]=info.getMimeType();decoder.setAllocator(ImageDecoder.ALLOCATOR_SOFTWARE);
                double pixels=(double)info.getSize().getWidth()*info.getSize().getHeight();
                decoder.setTargetSampleSize(Math.max(1,(int)Math.ceil(Math.sqrt(pixels/MAX_DECODE_PIXELS))));
                decoder.setOnPartialImageListener(error->false);
            });image.recycle();
        }catch(IOException|RuntimeException invalid){return null;}
        if(mime[0]==null)return null;
        boolean extensionMatches;
        switch(mime[0]){
            case "image/jpeg":extensionMatches=Arrays.asList("jpg","jpeg","jpe").contains(extension);break;
            case "image/png":extensionMatches="png".equals(extension);break;
            case "image/webp":extensionMatches="webp".equals(extension);break;
            case "image/gif":extensionMatches="gif".equals(extension);break;
            case "image/bmp":case "image/x-ms-bmp":extensionMatches="bmp".equals(extension);break;
            case "image/heif":case "image/heic":extensionMatches=Arrays.asList("heif","heic").contains(extension);break;
            case "image/avif":extensionMatches="avif".equals(extension);break;
            default:extensionMatches=false;
        }
        // ImageDecoder identifies HEIC as generic HEIF, but MediaStore otherwise appends .heif to .heic names.
        String galleryMime="heic".equals(extension)&&"image/heif".equals(mime[0])?"image/heic":mime[0];
        return extensionMatches?new Photo(file,galleryMime):null;
    }
    private static SharedPreferences journal(Context context){return context.getSharedPreferences(STATE_PREFS,Context.MODE_PRIVATE);}
    private static void remember(Context c,Uri uri,boolean add)throws IOException {
        SharedPreferences p=journal(c);Set<String> values=new HashSet<>(p.getStringSet(PENDING,Collections.emptySet()));if(add)values.add(uri.toString());else values.remove(uri.toString());
        if(!p.edit().putStringSet(PENDING,values).commit())throw new IOException("Gallery recovery journal unavailable");
    }
    // Only our journal's own pending rows may be removed after a process interruption; published/user photos stay intact.
    static synchronized void recover(Context context){
        Set<String> pending=new HashSet<>(journal(context).getStringSet(PENDING,Collections.emptySet()));
        // A crash can occur between insert and journaling. Recover only this app's unpublished rows in its own album.
        Uri collection=MediaStore.setIncludePending(MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY));
        try(Cursor cursor=context.getContentResolver().query(collection,new String[]{MediaStore.Images.Media._ID},MediaStore.MediaColumns.IS_PENDING+"=1 AND "+MediaStore.MediaColumns.OWNER_PACKAGE_NAME+"=? AND "+MediaStore.MediaColumns.RELATIVE_PATH+"=?",new String[]{context.getPackageName(),ALBUM_DIRECTORY},null)){
            if(cursor!=null)while(cursor.moveToNext())pending.add(ContentUris.withAppendedId(MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),cursor.getLong(0)).toString());
        }catch(RuntimeException ignored){}
        for(String value:pending){Uri uri=Uri.parse(value);boolean resolved=false;
            if(!"media".equals(uri.getAuthority())||!"content".equals(uri.getScheme()))continue;
            try(Cursor cursor=context.getContentResolver().query(uri,new String[]{MediaStore.MediaColumns.IS_PENDING,MediaStore.MediaColumns.OWNER_PACKAGE_NAME},null,null,null)){
                if(cursor==null||!cursor.moveToFirst())resolved=true;
                else if(context.getPackageName().equals(cursor.getString(1))){if(cursor.getInt(0)==1)context.getContentResolver().delete(uri,null,null);resolved=true;}
            }catch(RuntimeException ignored){}
            if(resolved)try{remember(context,uri,false);}catch(IOException ignored){}
        }
    }
    static synchronized Result save(Context context,List<Photo> photos,AtomicBoolean cancelled,Progress progress){
        recover(context);Result result=new Result();ContentResolver resolver=context.getContentResolver();long total=0,copied=0;for(Photo p:photos)total+=p.file.length();
        for(Photo selected:photos){if(cancelled.get()){result.cancelled=true;break;}Uri uri=null;boolean published=false;
            try{
                Photo photo=verified(NativeBridge.received,selected.file.getName());if(photo==null)throw new IOException("Photo no longer available");
                ContentValues values=new ContentValues();values.put(MediaStore.Images.Media.DISPLAY_NAME,photo.file.getName());values.put(MediaStore.Images.Media.MIME_TYPE,photo.mime);
                values.put(MediaStore.Images.Media.RELATIVE_PATH,ALBUM_DIRECTORY);values.put(MediaStore.Images.Media.IS_PENDING,1);
                // insert creates a new row and MediaStore chooses a collision-safe path; never update an existing image.
                uri=resolver.insert(MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),values);if(uri==null)throw new IOException("Gallery insertion failed");remember(context,uri,true);
                java.security.MessageDigest sourceHash=java.security.MessageDigest.getInstance("SHA-256");long size=0;
                try(InputStream input=new FileInputStream(photo.file);OutputStream output=resolver.openOutputStream(uri,"w")){
                    if(output==null)throw new IOException("Gallery output unavailable");byte[] buffer=new byte[BUFFER_BYTES];int n;
                    while((n=input.read(buffer))!=-1){if(cancelled.get())throw new InterruptedIOException();output.write(buffer,0,n);sourceHash.update(buffer,0,n);size+=n;copied+=n;progress.bytes(copied,total);}
                    output.flush();if(output instanceof FileOutputStream)((FileOutputStream)output).getFD().sync();
                }
                // Verify the stored copy before publication; successful transfer and successful gallery copy remain distinct.
                java.security.MessageDigest storedHash=java.security.MessageDigest.getInstance("SHA-256");long storedSize=0;
                try(InputStream input=resolver.openInputStream(uri)){if(input==null)throw new IOException("Gallery verification unavailable");byte[] buffer=new byte[BUFFER_BYTES];int n;while((n=input.read(buffer))!=-1){if(cancelled.get())throw new InterruptedIOException();storedHash.update(buffer,0,n);storedSize+=n;}}
                if(size!=photo.file.length()||storedSize!=size||!java.util.Arrays.equals(sourceHash.digest(),storedHash.digest()))throw new IOException("Gallery copy verification failed");
                if(cancelled.get())throw new InterruptedIOException();values.clear();values.put(MediaStore.Images.Media.IS_PENDING,0);
                if(resolver.update(uri,values,null,null)!=1)throw new IOException("Gallery publication failed");published=true;result.saved++;if(result.first==null){result.first=uri;result.firstMime=photo.mime;}remember(context,uri,false);
            }catch(InterruptedIOException stop){result.cancelled=true;}catch(Exception error){if(!published)result.failed++;}
            finally{if(uri!=null&&!published)try{resolver.delete(uri,null,null);remember(context,uri,false);}catch(Exception ignored){} }
            if(result.cancelled)break;
        }return result;
    }
}
