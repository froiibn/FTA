package app.fta.android;

import android.content.*;
import android.net.*;
import android.net.nsd.*;
import android.net.wifi.WifiManager;
import android.os.Build;
import org.json.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.concurrent.Executor;

// NSD is an address hint only: the Rust engine probes reachability and authenticates every registration/transfer.
final class LanDiscovery {
    static final String TRANSFER="_fta._tcp.", PAIRING="_fta-pair._tcp.";
    static final class Lan {
        final Network network; final String address,cidr,interfaceName,scope;
        Lan(Network n,String a,String c,String i){network=n;address=a;cidr=c;interfaceName=i;scope=i+"|"+c;}
        String identity(){return network+"|"+address+"|"+scope;}
    }
    private final NsdManager nsd; private final Executor worker,callbacks;
    private Network selectedNetwork;
    private final ArrayList<NsdManager.DiscoveryListener> browsers=new ArrayList<>();
    private final HashMap<String,NsdManager.RegistrationListener> publishers=new HashMap<>();
    private final HashMap<String,String> publishedKeys=new HashMap<>(), resolvedIDs=new HashMap<>();
    private WifiManager.MulticastLock multicast;
    private static final int MAX_SERVICES=1000;
    private volatile int generation;
    java.util.function.Consumer<String> failure;
    private final ArrayDeque<Runnable> resolveQueue=new ArrayDeque<>();
    private boolean resolving;
    LanDiscovery(Context context,Executor executor){nsd=context.getSystemService(NsdManager.class);worker=executor;callbacks=context.getMainExecutor();
        WifiManager wifi=(WifiManager)context.getApplicationContext().getSystemService(Context.WIFI_SERVICE);
        if(wifi!=null){multicast=wifi.createMulticastLock("FTA service discovery");multicast.setReferenceCounted(false);}}
    static Lan current(Context context){
        ConnectivityManager cm=context.getSystemService(ConnectivityManager.class);
        for(Network n:cm.getAllNetworks()){
            NetworkCapabilities caps=cm.getNetworkCapabilities(n);LinkProperties links=cm.getLinkProperties(n);
            if(caps==null||links==null||caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)||(!caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)&&!caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)))continue;
            for(LinkAddress link:links.getLinkAddresses()){
                if(!(link.getAddress() instanceof Inet4Address))continue;
                byte[] b=link.getAddress().getAddress();int v=((b[0]&255)<<24)|((b[1]&255)<<16)|((b[2]&255)<<8)|(b[3]&255);
                boolean local=(b[0]&255)==10 || ((b[0]&255)==172&&(b[1]&240)==16) || ((b[0]&255)==192&&(b[1]&255)==168);
                int bits=link.getPrefixLength();int privatePrefix=(b[0]&255)==10?8:(b[0]&255)==172?12:16;
                // A private interface must not grant a supernet that also includes public routed addresses.
                if(!local||bits<privatePrefix||bits>32)continue;
                int mask=-1<<(32-bits), base=v&mask;
                String cidr=((base>>>24)&255)+"."+((base>>>16)&255)+"."+((base>>>8)&255)+"."+(base&255)+"/"+bits;
                return new Lan(n,link.getAddress().getHostAddress(),cidr,links.getInterfaceName());
            }
        }return null;
    }
    void start(Lan lan){stop();selectedNetwork=lan.network;final int version=generation;
        if(multicast!=null&&!multicast.isHeld())multicast.acquire();
        for(String type:new String[]{TRANSFER,PAIRING}){
            NsdManager.DiscoveryListener listener=new NsdManager.DiscoveryListener(){
                public void onDiscoveryStarted(String t){} public void onDiscoveryStopped(String t){}
                public void onStartDiscoveryFailed(String t,int error){if(version==generation&&failure!=null)failure.accept("기기 검색을 시작하지 못했습니다. 로컬 네트워크 권한과 Wi-Fi를 확인하세요.");}public void onStopDiscoveryFailed(String t,int error){}
                public void onServiceFound(NsdServiceInfo found){
                    if(version!=generation||resolveQueue.size()+resolvedIDs.size()>=MAX_SERVICES)return;
                    // Older Android NSD allows one outstanding resolution. Queue instead of losing peers to ALREADY_ACTIVE.
                    resolveQueue.add(()->nsd.resolveService(found,new NsdManager.ResolveListener(){
                        public void onResolveFailed(NsdServiceInfo i,int error){callbacks.execute(()->resolved());}
                        public void onServiceResolved(NsdServiceInfo info){callbacks.execute(()->{
                            try{if(version==generation)accept(info,type,version);}finally{resolved();}
                        });}
                    }));drain();
                }
                public void onServiceLost(NsdServiceInfo info){if(version!=generation)return;String id=resolvedIDs.remove(type+info.getServiceName());if(id!=null)worker.execute(()->{if(version!=generation)return;try{NativeBridge.call("host.removed",new JSONObject().put("device_id",id).put("pairing",PAIRING.equals(type)));}catch(Exception ignored){}});}
            };
            browsers.add(listener);
            // Binding discovery to the chosen Wi-Fi/Ethernet network prevents a VPN/cellular route from inheriting consent.
            if(Build.VERSION.SDK_INT>=33)nsd.discoverServices(type,NsdManager.PROTOCOL_DNS_SD,lan.network,callbacks,listener);
            else nsd.discoverServices(type,NsdManager.PROTOCOL_DNS_SD,listener);
        }
    }
    private void drain(){if(resolving||resolveQueue.isEmpty())return;resolving=true;try{resolveQueue.removeFirst().run();}catch(RuntimeException e){resolving=false;resolveQueue.clear();if(failure!=null)failure.accept("기기 주소를 확인하지 못했습니다. 네트워크 권한과 연결을 확인하세요.");}}
    private void resolved(){resolving=false;drain();}
    private void accept(NsdServiceInfo info,String type,int version){
        Map<String,byte[]> attrs=info.getAttributes();String id=text(attrs,"device_id"),name=text(attrs,"display_name"),os=text(attrs,"os");
        try{UUID.fromString(id);}catch(Exception invalid){return;}
        if(!"1".equals(text(attrs,"version"))||name.getBytes(StandardCharsets.UTF_8).length>128||os.getBytes(StandardCharsets.UTF_8).length>32)return;
        List<InetAddress> hosts=Build.VERSION.SDK_INT>=34?info.getHostAddresses():Collections.singletonList(info.getHost());
        for(InetAddress host:hosts){if(!(host instanceof Inet4Address))continue;
            resolvedIDs.put(type+info.getServiceName(),id);
            worker.execute(()->{if(version!=generation)return;try{NativeBridge.call("host.discovered",new JSONObject().put("pairing",PAIRING.equals(type)).put("device",new JSONObject().put("device_id",id).put("display_name",name).put("os",os).put("address",host.getHostAddress()+":"+info.getPort())));}catch(Exception ignored){}});
        }
    }
    private static String text(Map<String,byte[]> attrs,String key){byte[] b=attrs.get(key);return b==null?"":new String(b,StandardCharsets.UTF_8);}
    void advertise(JSONObject advert,String name){for(String type:new String[]{TRANSFER,PAIRING}){
        String address=advert.optString(PAIRING.equals(type)?"pairing_address":"address","");
        if(address.isEmpty()||"null".equals(address)){removePublisher(type);continue;}
        String id=advert.optString("device_id");String key=id+"|"+address+"|"+name;
        if(key.equals(publishedKeys.get(type)))continue;removePublisher(type);
        int port;try{port=Integer.parseInt(address.substring(address.lastIndexOf(':')+1));}catch(Exception invalid){continue;}
        NsdServiceInfo info=new NsdServiceInfo();info.setServiceName(id);info.setServiceType(type);info.setPort(port);if(Build.VERSION.SDK_INT>=33)info.setNetwork(selectedNetwork);
        info.setAttribute("device_id",id);info.setAttribute("display_name",name);info.setAttribute("os","android");info.setAttribute("version","1");
        NsdManager.RegistrationListener registration=new NsdManager.RegistrationListener(){
            public void onRegistrationFailed(NsdServiceInfo s,int e){callbacks.execute(()->{if(publishers.get(type)==this){publishedKeys.remove(type);if(failure!=null)failure.accept("수신 서비스를 알리지 못했습니다. 로컬 네트워크 권한과 Wi-Fi를 확인하세요.");}});}public void onUnregistrationFailed(NsdServiceInfo s,int e){}
            public void onServiceRegistered(NsdServiceInfo s){}public void onServiceUnregistered(NsdServiceInfo s){}
        };publishers.put(type,registration);publishedKeys.put(type,key);nsd.registerService(info,NsdManager.PROTOCOL_DNS_SD,registration);
    }}
    private void removePublisher(String type){NsdManager.RegistrationListener listener=publishers.remove(type);publishedKeys.remove(type);if(listener!=null)try{nsd.unregisterService(listener);}catch(Exception ignored){}}
    void stop(){generation++;resolveQueue.clear();for(NsdManager.DiscoveryListener l:browsers)try{nsd.stopServiceDiscovery(l);}catch(Exception ignored){}browsers.clear();for(String t:new ArrayList<>(publishers.keySet()))removePublisher(t);resolvedIDs.clear();if(multicast!=null&&multicast.isHeld())multicast.release();}
}
