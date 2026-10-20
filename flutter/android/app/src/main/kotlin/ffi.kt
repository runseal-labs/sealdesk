// ffi.kt

package ffi

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.util.Log
import java.net.NetworkInterface
import java.nio.ByteBuffer
import org.json.JSONArray
import org.json.JSONObject

import com.runseal.sealdesk.RdClipboardManager

object FFI {
    init {
        System.loadLibrary("sealdesk")
    }

    external fun init(ctx: Context)
    external fun onAppStart(ctx: Context)
    external fun setClipboardManager(clipboardManager: RdClipboardManager)
    external fun startServer(app_dir: String, home_dir: String, custom_client_config: String)
    external fun setTailcatInterfaces(interfaces: String)
    external fun startService()
    external fun onVideoFrameUpdate(buf: ByteBuffer)
    external fun onAudioFrameUpdate(buf: ByteBuffer)
    external fun translateLocale(localeName: String, input: String): String
    external fun refreshScreen()
    external fun closeAllSessions()
    external fun setFrameRawEnable(name: String, value: Boolean)
    external fun setCodecInfo(info: String)
    external fun getLocalOption(key: String): String
    external fun getBuildinOption(key: String): String
    external fun onClipboardUpdate(clips: ByteBuffer)
    external fun isServiceClipboardEnabled(): Boolean

    // Android denies netlink to app UIDs, so the Go netmon cannot list interfaces itself.
    // This payload is the full list, including VPN up/down. Rust keeps a LAN copy
    // that still drops down, loopback, point-to-point, tun/ppp/wg, and any
    // interface tagged from a ConnectivityManager TRANSPORT_VPN network.
    fun pushTailcatInterfaces(context: Context) {
        val interfaces = try {
            NetworkInterface.getNetworkInterfaces()?.toList() ?: return
        } catch (e: Exception) {
            Log.w("FFI", "list network interfaces failed: $e")
            return
        }
        val vpnNames = vpnInterfaceNames(context)
        val array = JSONArray()
        for (nif in interfaces) {
            val name = nif.name ?: continue
            var flags = 0
            if (nif.isUp) flags = flags or 1
            if (nif.supportsMulticast()) flags = flags or 16
            if (nif.isLoopback) flags = flags or 4
            if (nif.isPointToPoint) flags = flags or 8
            val addrs = JSONArray()
            for (addr in nif.interfaceAddresses) {
                val host = addr.address.hostAddress?.substringBefore('%') ?: continue
                addrs.put("$host/${addr.networkPrefixLength}")
            }
            val obj = JSONObject()
                .put("name", name)
                .put("index", nif.index)
                .put("mtu", nif.mtu)
                .put("flags", flags)
                .put("addrs", addrs)
            if (isVpnInterfaceName(name) || vpnNames.contains(name)) {
                obj.put("vpn", true)
            }
            array.put(obj)
        }
        setTailcatInterfaces(array.toString())
    }

    private fun isVpnInterfaceName(name: String): Boolean {
        val n = name.lowercase(java.util.Locale.ROOT)
        return n.startsWith("tun") || n.startsWith("ppp") || n.startsWith("wg")
    }

    private fun vpnInterfaceNames(context: Context): Set<String> {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
            ?: return emptySet()
        val names = HashSet<String>()
        @Suppress("DEPRECATION")
        val networks = try {
            cm.allNetworks
        } catch (e: Exception) {
            Log.w("FFI", "list networks failed: $e")
            return emptySet()
        }
        for (network in networks) {
            val caps = try {
                cm.getNetworkCapabilities(network)
            } catch (e: Exception) {
                null
            } ?: continue
            if (!caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) continue
            val name = try {
                cm.getLinkProperties(network)?.interfaceName
            } catch (e: Exception) {
                null
            } ?: continue
            names.add(name)
        }
        return names
    }
}
