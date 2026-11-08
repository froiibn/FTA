package app.fta.android;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import javax.net.ssl.HttpsURLConnection;
import org.json.JSONException;
import org.json.JSONObject;

// Read only the public metadata selected by the desktop publisher.
enum DesktopDistribution {
    WINDOWS("windows-x86_64", "x64-setup.exe", "x64-setup.exe"),
    MAC("darwin-aarch64", "aarch64.app.tar.gz", "aarch64.dmg");

    static final String RELEASES = "https://github.com/froiibn/FTA/releases";
    private static final int MANIFEST_LIMIT = 64 * 1024;
    private static final int TIMEOUT_MS = 8_000;
    private final String target;
    private final String updateAsset;
    private final String installerAsset;

    DesktopDistribution(String target, String updateAsset, String installerAsset) {
        this.target = target;
        this.updateAsset = updateAsset;
        this.installerAsset = installerAsset;
    }

    String installerAddress() {
        HttpsURLConnection connection = null;
        try {
            String endpoint = RELEASES + "/latest/download/latest-" + target + ".json";
            connection = (HttpsURLConnection) URI.create(endpoint).toURL().openConnection();
            connection.setConnectTimeout(TIMEOUT_MS);
            connection.setReadTimeout(TIMEOUT_MS);
            if (connection.getResponseCode() != 200) return null;
            try (InputStream input = connection.getInputStream();
                 ByteArrayOutputStream bytes = new ByteArrayOutputStream()) {
                byte[] chunk = new byte[4096];
                int count;
                while ((count = input.read(chunk)) != -1) {
                    if (bytes.size() + count > MANIFEST_LIMIT) return null;
                    bytes.write(chunk, 0, count);
                }
                JSONObject manifest = new JSONObject(new String(bytes.toByteArray(), StandardCharsets.UTF_8));
                String version = manifest.getString("version");
                if (!version.matches("[0-9]+\\.[0-9]+\\.[0-9]+")
                        || manifest.getString("signature").isEmpty()) return null;
                // The Mac updater's tarball and manual-install DMG share the same release tag.
                String prefix = RELEASES + "/download/v" + version + "/FTA_" + version + "_";
                return manifest.getString("url").equals(prefix + updateAsset) ? prefix + installerAsset : null;
            }
        } catch (IOException | JSONException ignored) {
            // The releases list stays available in the guide when metadata cannot be read.
            return null;
        } finally {
            if (connection != null) connection.disconnect();
        }
    }
}
