// ffi.kt

package ffi

import android.content.Context
import android.util.Log
import java.net.NetworkInterface
import java.nio.ByteBuffer
import org.json.JSONArray
import org.json.JSONObject

import com.carriez.flutter_hbb.RdClipboardManager

object FFI {
    init {
        System.loadLibrary("rustdesk")
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
    fun pushTailcatInterfaces() {
        val interfaces = try {
            NetworkInterface.getNetworkInterfaces()?.toList() ?: return
        } catch (e: Exception) {
            Log.w("FFI", "list network interfaces failed: $e")
            return
        }
        val array = JSONArray()
        for (nif in interfaces) {
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
            array.put(
                JSONObject()
                    .put("name", nif.name)
                    .put("index", nif.index)
                    .put("mtu", nif.mtu)
                    .put("flags", flags)
                    .put("addrs", addrs)
            )
        }
        setTailcatInterfaces(array.toString())
    }
}
