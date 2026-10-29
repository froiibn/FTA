// JNI owns Java arrays; Rust owns JSON responses. Never reinterpret Java modified UTF-8 as message bytes.
#include <jni.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

enum { MAX_CONTROL_BYTES = 8 * 1024 * 1024 };
static JavaVM *vm;
static jclass bridge_class;
static jmethodID store_method, load_method;
extern char *fta_android_call(const char *);
extern void fta_android_free(char *);
extern int32_t fta_android_configure(const char *,const char *,const char *);

JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *machine, void *reserved) {
    (void)reserved;
    JNIEnv *env = NULL;
    if ((*machine)->GetEnv(machine,(void **)&env,JNI_VERSION_1_6) != JNI_OK) return JNI_ERR;
    jclass local = (*env)->FindClass(env,"app/fta/android/NativeBridge");
    if (!local) return JNI_ERR;
    bridge_class = (*env)->NewGlobalRef(env,local);
    (*env)->DeleteLocalRef(env,local);
    store_method = (*env)->GetStaticMethodID(env,bridge_class,"storeSecret","(Ljava/lang/String;[B)V");
    load_method = (*env)->GetStaticMethodID(env,bridge_class,"loadSecret","(Ljava/lang/String;)[B");
    if (!store_method || !load_method) return JNI_ERR;
    vm = machine;
    return JNI_VERSION_1_6;
}

JNIEXPORT jbyteArray JNICALL Java_app_fta_android_NativeBridge_nativeCall(JNIEnv *env,jclass cls,jbyteArray input) {
    (void)cls;
    jsize length = (*env)->GetArrayLength(env,input);
    // Controls contain paths/metadata only. The engine enforces its own message/manifest limits too.
    if (length > MAX_CONTROL_BYTES) return NULL;
    char *request = calloc((size_t)length+1,1);
    if (!request) return NULL;
    (*env)->GetByteArrayRegion(env,input,0,length,(jbyte *)request);
    char *response = fta_android_call(request);
    free(request);
    if (!response) return NULL;
    // Bound the Java allocation too; event consumers page messages separately to stay within this control budget.
    size_t response_size = strlen(response);
    if (response_size > MAX_CONTROL_BYTES) { fta_android_free(response); return NULL; }
    jsize size = (jsize)response_size;
    jbyteArray result = (*env)->NewByteArray(env,size);
    if (result) (*env)->SetByteArrayRegion(env,result,0,size,(const jbyte *)response);
    fta_android_free(response);
    return result;
}
JNIEXPORT jint JNICALL Java_app_fta_android_NativeBridge_nativeConfigure(JNIEnv *env,jclass cls,jstring root,jstring state,jstring received) {
    (void)cls;
    const char *r=(*env)->GetStringUTFChars(env,root,NULL), *s=(*env)->GetStringUTFChars(env,state,NULL), *d=(*env)->GetStringUTFChars(env,received,NULL);
    int result = r && s && d ? fta_android_configure(r,s,d) : -1;
    if(r)(*env)->ReleaseStringUTFChars(env,root,r);
    if(s)(*env)->ReleaseStringUTFChars(env,state,s);
    if(d)(*env)->ReleaseStringUTFChars(env,received,d);
    return result;
}
static JNIEnv *attach(int *attached) {
    JNIEnv *env = NULL; *attached=0;
    jint status=(*vm)->GetEnv(vm,(void **)&env,JNI_VERSION_1_6);
    if(status==JNI_EDETACHED){if((*vm)->AttachCurrentThread(vm,&env,NULL)!=JNI_OK)return NULL;*attached=1;}
    else if(status!=JNI_OK)return NULL;
    return env;
}
// Clear exceptions without describing them: stack traces must not expose key material or message bodies.
static int failed(JNIEnv *env) { if((*env)->ExceptionCheck(env)){(*env)->ExceptionClear(env);return 1;}return 0; }
int32_t fta_android_store_key(const char *account,const uint8_t *secret,size_t size) {
    int attached=0; JNIEnv *env=attach(&attached); if(!env)return -1;
    jstring name=(*env)->NewStringUTF(env,account);
    jbyteArray key=(*env)->NewByteArray(env,(jsize)size);
    int result=-1;
    if(name && key){(*env)->SetByteArrayRegion(env,key,0,(jsize)size,(const jbyte *)secret);(*env)->CallStaticVoidMethod(env,bridge_class,store_method,name,key);result=failed(env)?-1:0;}
    if(name)(*env)->DeleteLocalRef(env,name);if(key)(*env)->DeleteLocalRef(env,key);
    if(attached)(*vm)->DetachCurrentThread(vm);return result;
}
intptr_t fta_android_load_key(const char *account,uint8_t *output,size_t capacity) {
    int attached=0; JNIEnv *env=attach(&attached);if(!env)return -1;
    jstring name=(*env)->NewStringUTF(env,account);
    jbyteArray key=name ? (*env)->CallStaticObjectMethod(env,bridge_class,load_method,name) : NULL;
    intptr_t result=-1;
    if(!failed(env) && key){jsize size=(*env)->GetArrayLength(env,key);if(size>0 && (size_t)size<=capacity){(*env)->GetByteArrayRegion(env,key,0,size,(jbyte *)output);result=failed(env)?-1:size;}
        // Overwrite the temporary Java key copy after JNI copied it into the engine's owned buffer.
        jbyte zero[16384]={0};if(size<=16384)(*env)->SetByteArrayRegion(env,key,0,size,zero);
    }
    if(key)(*env)->DeleteLocalRef(env,key);if(name)(*env)->DeleteLocalRef(env,name);
    if(attached)(*vm)->DetachCurrentThread(vm);return result;
}
