package dev.plume.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ExportFormatTest {
    @Test
    fun extensions_are_unique_and_lowercase() {
        val extensions = ExportFormat.entries.map { it.extension }
        assertEquals(extensions.size, extensions.toSet().size)
        extensions.forEach { extension ->
            assertEquals(extension.lowercase(), extension)
            assertTrue(extension.isNotBlank())
        }
    }

    @Test
    fun mime_types_are_present_and_distinct_from_blank() {
        ExportFormat.entries.forEach { format ->
            assertTrue("${format.label} needs a MIME type", format.mimeType.contains('/'))
            assertNotEquals("", format.detail)
        }
    }

    @Test
    fun all_advertised_exporters_are_reachable_from_the_sheet() {
        val extensions = ExportFormat.entries.map { it.extension }.toSet()
        assertEquals(
            setOf("plume", "feather", "glb", "gltf", "obj", "stl", "usdz", "png"),
            extensions,
        )
    }
}
