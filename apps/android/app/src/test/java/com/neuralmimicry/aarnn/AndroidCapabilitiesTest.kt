package com.neuralmimicry.aarnn

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidCapabilitiesTest {
    @Test
    fun missingNativeLibraryDoesNotAdvertiseHardwareOrGlobalHid() {
        val report = AndroidCapabilities.report(nativeLibraryAvailable = false)

        assertFalse(report.getValue("usb_aer_input").available)
        assertFalse(report.getValue("usb_aer_output").available)
        assertFalse(report.getValue("native_global_hid").available)
        assertFalse(report.getValue("camera_capture").available)
    }

    @Test
    fun nativeLibraryAvailabilityOnlyEnablesReferenceAbi() {
        val report = AndroidCapabilities.report(nativeLibraryAvailable = true)

        assertTrue(report.getValue("local_standalone_reference").available)
        assertFalse(report.getValue("remote_management").available)
        assertFalse(report.getValue("background_execution").available)
    }

    @Test
    fun missingNativeLibraryCannotCreateOrBackgroundASession() {
        val runtime = AndroidRuntimeController()

        assertFalse(runtime.create(1L, "{}"))
        assertFalse(runtime.onBackground())
        assertFalse(runtime.onForeground())
        assertFalse(runtime.pause())
    }
}
