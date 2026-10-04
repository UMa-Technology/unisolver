/// The database format this engine reads: tetra3's format 2, whose files start with `T3DB` and
/// the version (2, little-endian). Databases from earlier releases (the UNISOLV2 container) do
/// not load; installers replace them.
library;

import 'dart:io';

/// First bytes of a format-2 database
const List<int> format2Header = [0x54, 0x33, 0x44, 0x42, 2, 0];

/// Whether [f] holds a database this engine reads, judged by its header
bool isFormat2File(File f) {
  if (!f.existsSync() || f.lengthSync() < format2Header.length) return false;
  final raf = f.openSync();
  try {
    final head = raf.readSync(format2Header.length);
    for (var i = 0; i < format2Header.length; i++) {
      if (head[i] != format2Header[i]) return false;
    }
    return true;
  } finally {
    raf.closeSync();
  }
}
