"""Published spreadsheets may omit printed headings (#1348)."""

import os
import sys
import tempfile
import unittest

try:
    import numpy as np
    from PIL import Image
except ImportError:
    np = None

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "tools", "render-lab"))


@unittest.skipIf(np is None, "numpy and Pillow are the render lab's")
class UnprintedHeadingsTest(unittest.TestCase):
    def aligned(self, factor=1, moved=False):
        import compare

        cells = np.full((80, 120, 3), 255, dtype=np.uint8)
        cells[10:18, 10:35] = 0
        cells[40:48, 10:25] = 0
        cells[40:48, 50:85] = 0
        ref = Image.new("RGB", (220, 180), "white")
        ref.paste(Image.fromarray(cells), (30, 40))
        if moved:
            cells[40:48, 50:85] = 255
            cells[55:63, 50:85] = 0
        cells = Image.fromarray(cells).resize((120 * factor, 80 * factor), Image.Resampling.NEAREST)
        ours = Image.new("RGB", (180 * factor + 40, 110 * factor + 20), "white")
        ours.paste((240, 240, 240), (0, 0, ours.width, 20))
        ours.paste((240, 240, 240), (0, 0, 40, ours.height))
        ours.paste(cells, (40, 20))
        with tempfile.TemporaryDirectory() as directory:
            rp, op = os.path.join(directory, "ref.png"), os.path.join(directory, "ours.png")
            ref.save(rp)
            ours.save(op)
            return compare.align("tables", rp, op)

    def test_same_cells_ignore_screen_headers_and_unused_padding(self):
        ref, ours, scale = self.aligned()
        self.assertEqual(scale, 1)
        np.testing.assert_array_equal(np.asarray(ref), np.asarray(ours))

    def test_actual_scale_difference_is_still_measured(self):
        _, _, scale = self.aligned(factor=2)
        self.assertEqual(scale, 2)

    def test_internal_layout_difference_remains_visible(self):
        ref, ours, _ = self.aligned(moved=True)
        self.assertFalse(np.array_equal(np.asarray(ref), np.asarray(ours)))
