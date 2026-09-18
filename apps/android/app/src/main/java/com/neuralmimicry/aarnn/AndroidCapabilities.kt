package com.neuralmimicry.aarnn

/**
 * Android capability reporting is deliberately conservative.  Discovering a
 * permission, USB port or network route does not grant access to a brain.
 * Each capability must later be enabled by its independently authorised
 * adapter and session.
 */
data class CapabilityStatus(val available: Boolean, val reason: String)

object AndroidCapabilities {
    fun report(nativeLibraryAvailable: Boolean): Map<String, CapabilityStatus> =
        linkedMapOf(
            "local_standalone_reference" to CapabilityStatus(
                nativeLibraryAvailable,
                if (nativeLibraryAvailable) {
                    "Rust ABI is present; production standalone acceptance is still required"
                } else {
                    "Rust Android ABI is not packaged"
                },
            ),
            "remote_management" to unavailable("generated management client is not packaged"),
            "foreground_edge_execution" to unavailable("Android foreground adapter is not enabled"),
            "camera_capture" to unavailable("governed camera-to-AER admission is not enabled; camera_preview remains available"),
            "camera_preview" to CapabilityStatus(true, "Native preview dialog; governed camera admission remains separately authorised"),
            "video_file_preview" to CapabilityStatus(true, "Native video picker and pop-out preview"),
            "video-file" to CapabilityStatus(true, "Selected source vocabulary: video-file; preview only until governed admission is enabled"),
            "camera" to CapabilityStatus(true, "Selected source vocabulary: camera; preview only until governed admission is enabled"),
            "microphone_capture" to unavailable("microphone adapter and consent flow are not enabled"),
            "local_network_discovery" to unavailable("authenticated NSD adapter is not enabled"),
            "usb_aer_input" to unavailable("USB Host AER adapter is not enabled"),
            "usb_aer_output" to unavailable("USB Host AER output is not enabled"),
            "background_execution" to unavailable("background execution requires a reviewed policy"),
            "native_global_hid" to unavailable("global HID is intentionally unavailable"),
        )

    private fun unavailable(reason: String) = CapabilityStatus(false, reason)
}
