#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Keys(&'static [u16]),
    Guide,
}

#[derive(Clone, Copy, Debug)]
pub enum Line {
    Sep,
    Cmd { id: &'static str, en: &'static str, vi: &'static str, accel: &'static str, act: Act },
}

#[derive(Clone, Copy, Debug)]
pub struct Menu {
    pub id: &'static str,
    pub en: &'static str,
    pub vi: &'static str,
    pub lines: &'static [Line],
}

impl Menu {
    pub fn title(&self, lang: crate::i18n::Lang) -> &'static str {
        match lang {
            crate::i18n::Lang::Id => self.id,
            crate::i18n::Lang::En => self.en,
            crate::i18n::Lang::Vi => self.vi,
        }
    }
}

impl Line {
    pub fn label(&self, lang: crate::i18n::Lang) -> &'static str {
        match self {
            Line::Sep => "",
            Line::Cmd { id, en, vi, .. } => match lang {
                crate::i18n::Lang::Id => id,
                crate::i18n::Lang::En => en,
                crate::i18n::Lang::Vi => vi,
            },
        }
    }
}

const C: u16 = 0x11;
const S: u16 = 0x10;
const A: u16 = 0x12;
const W: u16 = 0x5B;
const TAB: u16 = 0x09;
const LEFT: u16 = 0x25;
const UP: u16 = 0x26;
const RIGHT: u16 = 0x27;
const DOWN: u16 = 0x28;
const DEL: u16 = 0x2E;
const ENTER: u16 = 0x0D;
const PGUP: u16 = 0x21;
const PGDN: u16 = 0x22;
const PLUS: u16 = 0xBB;
const MINUS: u16 = 0xBD;
const SLASH: u16 = 0xBF;
const TICK: u16 = 0xC0;
const SPACE: u16 = 0x20;
const F1: u16 = 0x70;
const F2: u16 = 0x71;
const F4: u16 = 0x73;
const F5: u16 = 0x74;
const F11: u16 = 0x7A;
const F12: u16 = 0x7B;

const fn k(id: &'static str, en: &'static str, vi: &'static str, accel: &'static str, keys: &'static [u16]) -> Line {
    Line::Cmd { id, en, vi, accel, act: Act::Keys(keys) }
}

const UNDO: Line = k("Urungkan", "Undo", "Hoàn tác", "Ctrl+Z", &[C, 0x5A]);
const REDO: Line = k("Ulangi", "Redo", "Làm lại", "Ctrl+Y", &[C, 0x59]);
const CUT: Line = k("Potong", "Cut", "Cắt", "Ctrl+X", &[C, 0x58]);
const COPY: Line = k("Salin", "Copy", "Sao chép", "Ctrl+C", &[C, 0x43]);
const PASTE: Line = k("Tempel", "Paste", "Dán", "Ctrl+V", &[C, 0x56]);
const ALL: Line = k("Pilih Semua", "Select All", "Chọn tất cả", "Ctrl+A", &[C, 0x41]);
const FIND: Line = k("Cari…", "Find…", "Tìm kiếm…", "Ctrl+F", &[C, 0x46]);
const ZOOM_IN: Line = k("Perbesar", "Zoom In", "Phóng to", "Ctrl++", &[C, PLUS]);
const ZOOM_OUT: Line = k("Perkecil", "Zoom Out", "Thu nhỏ", "Ctrl+-", &[C, MINUS]);
const ZOOM_0: Line = k("Ukuran Asli", "Actual Size", "Kích thước gốc", "Ctrl+0", &[C, 0x30]);
const FULL: Line = k("Layar Penuh", "Full Screen", "Toàn màn hình", "F11", &[F11]);
const BACK: Line = k("Mundur", "Back", "Quay lại", "Alt+←", &[A, LEFT]);
const FWD: Line = k("Maju", "Forward", "Tiếp theo", "Alt+→", &[A, RIGHT]);

const WINDOW: Menu = Menu {
    id: "Jendela",
    en: "Window",
    vi: "Cửa sổ",
    lines: &[
        k("Minimalkan", "Minimize", "Thu nhỏ", "Win+↓", &[W, DOWN]),
        k("Zoom", "Zoom", "Thu phóng", "Win+↑", &[W, UP]),
        Line::Sep,
        k("Pindah ke Kiri Layar", "Move to Left of Screen", "Chuyển sang trái màn hình", "Win+←", &[W, LEFT]),
        k("Pindah ke Kanan Layar", "Move to Right of Screen", "Chuyển sang phải màn hình", "Win+→", &[W, RIGHT]),
        Line::Sep,
        k("Tutup Jendela", "Close Window", "Đóng cửa sổ", "Alt+F4", &[A, F4]),
    ],
};

const HELP: Menu = Menu {
    id: "Bantuan",
    en: "Help",
    vi: "Trợ giúp",
    lines: &[
        Line::Cmd { id: "Panduan ABRA", en: "ABRA Guide", vi: "Hướng dẫn ABRA", accel: "", act: Act::Guide },
        Line::Sep,
        k("Bantuan Aplikasi", "App Help", "Trợ giúp ứng dụng", "F1", &[F1]),
    ],
};

const GENERIC: &[Menu] = &[
    Menu {
        id: "Berkas",
        en: "File",
        vi: "Tệp",
        lines: &[
            k("Baru", "New", "Mới", "Ctrl+N", &[C, 0x4E]),
            k("Buka…", "Open…", "Mở…", "Ctrl+O", &[C, 0x4F]),
            Line::Sep,
            k("Simpan", "Save", "Lưu", "Ctrl+S", &[C, 0x53]),
            k("Tutup", "Close", "Đóng", "Ctrl+W", &[C, 0x57]),
            Line::Sep,
            k("Cetak…", "Print…", "In…", "Ctrl+P", &[C, 0x50]),
        ],
    },
    Menu { id: "Edit", en: "Edit", vi: "Chỉnh sửa", lines: &[UNDO, REDO, Line::Sep, CUT, COPY, PASTE, ALL, Line::Sep, FIND] },
    Menu { id: "Tampilan", en: "View", vi: "Xem", lines: &[FULL, Line::Sep, ZOOM_IN, ZOOM_OUT, ZOOM_0] },
    WINDOW,
    HELP,
];

const BROWSER: &[Menu] = &[
    Menu {
        id: "Berkas",
        en: "File",
        vi: "Tệp",
        lines: &[
            k("Tab Baru", "New Tab", "Thẻ mới", "Ctrl+T", &[C, 0x54]),
            k("Jendela Baru", "New Window", "Cửa sổ mới", "Ctrl+N", &[C, 0x4E]),
            k("Jendela Penyamaran Baru", "New Incognito Window", "Cửa sổ ẩn danh mới", "Ctrl+Shift+N", &[C, S, 0x4E]),
            k("Buka Lagi Tab yang Ditutup", "Reopen Closed Tab", "Mở lại thẻ đã đóng", "Ctrl+Shift+T", &[C, S, 0x54]),
            Line::Sep,
            k("Buka Berkas…", "Open File…", "Mở tệp…", "Ctrl+O", &[C, 0x4F]),
            k("Simpan Halaman…", "Save Page As…", "Lưu trang thành…", "Ctrl+S", &[C, 0x53]),
            k("Cetak…", "Print…", "In…", "Ctrl+P", &[C, 0x50]),
            Line::Sep,
            k("Tutup Tab", "Close Tab", "Đóng thẻ", "Ctrl+W", &[C, 0x57]),
            k("Tutup Jendela", "Close Window", "Đóng cửa sổ", "Ctrl+Shift+W", &[C, S, 0x57]),
        ],
    },
    Menu {
        id: "Edit",
        en: "Edit",
        vi: "Chỉnh sửa",
        lines: &[UNDO, REDO, Line::Sep, CUT, COPY, PASTE, k("Tempel Teks Polos", "Paste as Plain Text", "Dán văn bản thuần", "Ctrl+Shift+V", &[C, S, 0x56]), ALL, Line::Sep, FIND],
    },
    Menu {
        id: "Tampilan",
        en: "View",
        vi: "Xem",
        lines: &[
            k("Muat Ulang", "Reload", "Tải lại", "Ctrl+R", &[C, 0x52]),
            k("Muat Ulang Penuh", "Hard Reload", "Tải lại hoàn toàn", "Ctrl+Shift+R", &[C, S, 0x52]),
            Line::Sep,
            FULL,
            ZOOM_IN,
            ZOOM_OUT,
            ZOOM_0,
            Line::Sep,
            k("Alat Pengembang", "Developer Tools", "Công cụ cho nhà phát triển", "Ctrl+Shift+I", &[C, S, 0x49]),
            k("Lihat Sumber", "View Source", "Xem mã nguồn", "Ctrl+U", &[C, 0x55]),
        ],
    },
    Menu {
        id: "Riwayat",
        en: "History",
        vi: "Lịch sử",
        lines: &[
            BACK,
            FWD,
            Line::Sep,
            k("Tampilkan Riwayat", "Show History", "Hiển thị lịch sử", "Ctrl+H", &[C, 0x48]),
            k("Unduhan", "Downloads", "Tải xuống", "Ctrl+J", &[C, 0x4A]),
            k("Hapus Data Penjelajahan…", "Clear Browsing Data…", "Xóa dữ liệu duyệt web…", "Ctrl+Shift+Del", &[C, S, DEL]),
        ],
    },
    Menu {
        id: "Bookmark",
        en: "Bookmarks",
        vi: "Dấu trang",
        lines: &[
            k("Bookmark Tab Ini", "Bookmark This Tab", "Đánh dấu thẻ này", "Ctrl+D", &[C, 0x44]),
            k("Bookmark Semua Tab", "Bookmark All Tabs", "Đánh dấu tất cả thẻ", "Ctrl+Shift+D", &[C, S, 0x44]),
            Line::Sep,
            k("Tampilkan Bilah Bookmark", "Show Bookmarks Bar", "Hiện thanh Dấu trang", "Ctrl+Shift+B", &[C, S, 0x42]),
            k("Pengelola Bookmark", "Bookmark Manager", "Trình quản lý Dấu trang", "Ctrl+Shift+O", &[C, S, 0x4F]),
        ],
    },
    Menu {
        id: "Tab",
        en: "Tab",
        vi: "Thẻ",
        lines: &[
            k("Tab Berikutnya", "Next Tab", "Thẻ tiếp theo", "Ctrl+Tab", &[C, TAB]),
            k("Tab Sebelumnya", "Previous Tab", "Thẻ trước đó", "Ctrl+Shift+Tab", &[C, S, TAB]),
            Line::Sep,
            k("Geser Tab ke Kiri", "Move Tab Left", "Chuyển thẻ sang trái", "Ctrl+Shift+PgUp", &[C, S, PGUP]),
            k("Geser Tab ke Kanan", "Move Tab Right", "Chuyển thẻ sang phải", "Ctrl+Shift+PgDn", &[C, S, PGDN]),
            Line::Sep,
            k("Tab Pertama", "First Tab", "Thẻ đầu tiên", "Ctrl+1", &[C, 0x31]),
            k("Tab Terakhir", "Last Tab", "Thẻ cuối cùng", "Ctrl+9", &[C, 0x39]),
            Line::Sep,
            k("Tutup Tab", "Close Tab", "Đóng thẻ", "Ctrl+W", &[C, 0x57]),
        ],
    },
    WINDOW,
    HELP,
];

const EXPLORER: &[Menu] = &[
    Menu {
        id: "Berkas",
        en: "File",
        vi: "Tệp",
        lines: &[
            k("Jendela Baru", "New Window", "Cửa sổ mới", "Ctrl+N", &[C, 0x4E]),
            k("Tab Baru", "New Tab", "Thẻ mới", "Ctrl+T", &[C, 0x54]),
            k("Folder Baru", "New Folder", "Thư mục mới", "Ctrl+Shift+N", &[C, S, 0x4E]),
            Line::Sep,
            k("Properti", "Properties", "Thuộc tính", "Alt+Enter", &[A, ENTER]),
            Line::Sep,
            k("Tutup Tab", "Close Tab", "Đóng thẻ", "Ctrl+W", &[C, 0x57]),
        ],
    },
    Menu {
        id: "Edit",
        en: "Edit",
        vi: "Chỉnh sửa",
        lines: &[
            UNDO,
            REDO,
            Line::Sep,
            CUT,
            COPY,
            PASTE,
            k("Salin Jalur", "Copy as Path", "Sao chép đường dẫn", "Ctrl+Shift+C", &[C, S, 0x43]),
            Line::Sep,
            k("Ganti Nama", "Rename", "Đổi tên", "F2", &[F2]),
            k("Hapus", "Delete", "Xóa", "Del", &[DEL]),
            Line::Sep,
            ALL,
        ],
    },
    Menu {
        id: "Tampilan",
        en: "View",
        vi: "Xem",
        lines: &[
            k("Segarkan", "Refresh", "Làm mới", "F5", &[F5]),
            Line::Sep,
            k("Ikon Besar", "Large Icons", "Biểu tượng lớn", "Ctrl+Shift+2", &[C, S, 0x32]),
            k("Ikon Sedang", "Medium Icons", "Biểu tượng vừa", "Ctrl+Shift+3", &[C, S, 0x33]),
            k("Ikon Kecil", "Small Icons", "Biểu tượng nhỏ", "Ctrl+Shift+4", &[C, S, 0x34]),
            k("Daftar", "List", "Danh sách", "Ctrl+Shift+5", &[C, S, 0x35]),
            k("Detail", "Details", "Chi tiết", "Ctrl+Shift+6", &[C, S, 0x36]),
            k("Ubin", "Tiles", "Ô xếp", "Ctrl+Shift+7", &[C, S, 0x37]),
            Line::Sep,
            k("Panel Pratinjau", "Preview Pane", "Khung xem trước", "Alt+P", &[A, 0x50]),
            k("Panel Detail", "Details Pane", "Khung chi tiết", "Alt+Shift+P", &[A, S, 0x50]),
        ],
    },
    Menu {
        id: "Buka",
        en: "Go",
        vi: "Đi tới",
        lines: &[
            BACK,
            FWD,
            k("Folder Induk", "Enclosing Folder", "Thư mục cha", "Alt+↑", &[A, UP]),
            Line::Sep,
            k("Bilah Alamat", "Address Bar", "Thanh địa chỉ", "Alt+D", &[A, 0x44]),
            k("Cari", "Search", "Tìm kiếm", "Ctrl+E", &[C, 0x45]),
        ],
    },
    WINDOW,
    HELP,
];

const OFFICE: &[Menu] = &[
    Menu {
        id: "Berkas",
        en: "File",
        vi: "Tệp",
        lines: &[
            k("Baru", "New", "Mới", "Ctrl+N", &[C, 0x4E]),
            k("Buka…", "Open…", "Mở…", "Ctrl+O", &[C, 0x4F]),
            Line::Sep,
            k("Simpan", "Save", "Lưu", "Ctrl+S", &[C, 0x53]),
            k("Simpan Sebagai…", "Save As…", "Lưu thành…", "F12", &[F12]),
            Line::Sep,
            k("Cetak…", "Print…", "In…", "Ctrl+P", &[C, 0x50]),
            k("Tutup", "Close", "Đóng", "Ctrl+W", &[C, 0x57]),
        ],
    },
    Menu {
        id: "Edit",
        en: "Edit",
        vi: "Chỉnh sửa",
        lines: &[UNDO, REDO, Line::Sep, CUT, COPY, PASTE, ALL, Line::Sep, FIND, k("Ganti…", "Replace…", "Thay thế…", "Ctrl+H", &[C, 0x48])],
    },
    Menu {
        id: "Format",
        en: "Format",
        vi: "Định dạng",
        lines: &[
            k("Tebal", "Bold", "In đậm", "Ctrl+B", &[C, 0x42]),
            k("Miring", "Italic", "In nghiêng", "Ctrl+I", &[C, 0x49]),
            k("Garis Bawah", "Underline", "Gạch chân", "Ctrl+U", &[C, 0x55]),
            Line::Sep,
            k("Rata Kiri", "Align Left", "Căn trái", "Ctrl+L", &[C, 0x4C]),
            k("Rata Tengah", "Center", "Căn giữa", "Ctrl+E", &[C, 0x45]),
            k("Rata Kanan", "Align Right", "Căn phải", "Ctrl+R", &[C, 0x52]),
        ],
    },
    WINDOW,
    HELP,
];

const CODE: &[Menu] = &[
    Menu {
        id: "Berkas",
        en: "File",
        vi: "Tệp",
        lines: &[
            k("Berkas Baru", "New File", "Tệp mới", "Ctrl+N", &[C, 0x4E]),
            k("Jendela Baru", "New Window", "Cửa sổ mới", "Ctrl+Shift+N", &[C, S, 0x4E]),
            k("Buka Berkas…", "Open File…", "Mở tệp…", "Ctrl+O", &[C, 0x4F]),
            Line::Sep,
            k("Simpan", "Save", "Lưu", "Ctrl+S", &[C, 0x53]),
            k("Simpan Sebagai…", "Save As…", "Lưu thành…", "Ctrl+Shift+S", &[C, S, 0x53]),
            Line::Sep,
            k("Tutup Editor", "Close Editor", "Đóng trình soạn thảo", "Ctrl+W", &[C, 0x57]),
        ],
    },
    Menu {
        id: "Edit",
        en: "Edit",
        vi: "Chỉnh sửa",
        lines: &[
            UNDO,
            REDO,
            Line::Sep,
            CUT,
            COPY,
            PASTE,
            Line::Sep,
            FIND,
            k("Ganti…", "Replace…", "Thay thế…", "Ctrl+H", &[C, 0x48]),
            k("Cari di Berkas…", "Find in Files…", "Tìm trong tệp…", "Ctrl+Shift+F", &[C, S, 0x46]),
            Line::Sep,
            k("Jadikan Komentar", "Toggle Comment", "Bật/tắt chú thích", "Ctrl+/", &[C, SLASH]),
        ],
    },
    Menu {
        id: "Tampilan",
        en: "View",
        vi: "Xem",
        lines: &[
            k("Palet Perintah…", "Command Palette…", "Bảng lệnh…", "Ctrl+Shift+P", &[C, S, 0x50]),
            k("Buka Cepat…", "Quick Open…", "Mở nhanh…", "Ctrl+P", &[C, 0x50]),
            Line::Sep,
            k("Penjelajah", "Explorer", "Trình khám phá", "Ctrl+Shift+E", &[C, S, 0x45]),
            k("Sidebar", "Toggle Sidebar", "Bật/tắt thanh bên", "Ctrl+B", &[C, 0x42]),
            k("Terminal", "Terminal", "Terminal", "Ctrl+`", &[C, TICK]),
            Line::Sep,
            ZOOM_IN,
            ZOOM_OUT,
            FULL,
        ],
    },
    Menu {
        id: "Buka",
        en: "Go",
        vi: "Đi tới",
        lines: &[
            k("Ke Baris…", "Go to Line…", "Tới dòng…", "Ctrl+G", &[C, 0x47]),
            k("Ke Simbol…", "Go to Symbol…", "Tới ký hiệu…", "Ctrl+Shift+O", &[C, S, 0x4F]),
            Line::Sep,
            BACK,
            FWD,
        ],
    },
    Menu {
        id: "Jalankan",
        en: "Run",
        vi: "Chạy",
        lines: &[
            k("Mulai Debug", "Start Debugging", "Bắt đầu gỡ lỗi", "F5", &[F5]),
            k("Jalankan Tanpa Debug", "Run Without Debugging", "Chạy không gỡ lỗi", "Ctrl+F5", &[C, F5]),
        ],
    },
    WINDOW,
    HELP,
];

const MUSIC: &[Menu] = &[
    Menu {
        id: "Putar",
        en: "Playback",
        vi: "Phát nhạc",
        lines: &[
            k("Putar / Jeda", "Play / Pause", "Phát / Tạm dừng", "Space", &[SPACE]),
            k("Berikutnya", "Next", "Bài tiếp theo", "Ctrl+→", &[C, RIGHT]),
            k("Sebelumnya", "Previous", "Bài trước", "Ctrl+←", &[C, LEFT]),
            Line::Sep,
            k("Naikkan Volume", "Volume Up", "Tăng âm lượng", "Ctrl+↑", &[C, UP]),
            k("Turunkan Volume", "Volume Down", "Giảm âm lượng", "Ctrl+↓", &[C, DOWN]),
        ],
    },
    Menu { id: "Edit", en: "Edit", vi: "Chỉnh sửa", lines: &[CUT, COPY, PASTE, ALL, Line::Sep, k("Cari", "Search", "Tìm kiếm", "Ctrl+L", &[C, 0x4C])] },
    WINDOW,
    HELP,
];

pub fn exe_of(path: &str) -> String {
    let lower = path.to_lowercase();
    lower.rsplit(['\\', '/']).next().unwrap_or("").to_string()
}

pub fn for_path(path: &str) -> &'static [Menu] {
    match exe_of(path).as_str() {
        "chrome.exe" | "msedge.exe" | "brave.exe" | "firefox.exe" | "opera.exe" | "vivaldi.exe" | "arc.exe" | "zen.exe" | "librewolf.exe" | "thorium.exe" => BROWSER,
        "explorer.exe" => EXPLORER,
        "winword.exe" | "excel.exe" | "powerpnt.exe" | "onenote.exe" => OFFICE,
        "code.exe" | "cursor.exe" | "windsurf.exe" | "code - insiders.exe" | "vscodium.exe" | "antigravity.exe" | "zed.exe" => CODE,
        "spotify.exe" => MUSIC,
        _ => GENERIC,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_follow_the_app() {
        assert_eq!(for_path(r"C:\Program Files\Google\Chrome\Application\chrome.exe")[0].lines.len(), 11);
        assert_eq!(for_path(r"C:\Windows\explorer.exe")[3].en, "Go");
        assert_eq!(for_path(r"C:\Apps\WINWORD.EXE")[2].en, "Format");
        assert_eq!(for_path("shell:AppsFolder\\Something!App")[0].en, "File");
    }

    #[test]
    fn every_profile_ends_with_window_and_help() {
        for exe in ["chrome.exe", "explorer.exe", "winword.exe", "code.exe", "spotify.exe", "notepad.exe"] {
            let m = for_path(exe);
            assert_eq!(m[m.len() - 2].en, "Window");
            assert_eq!(m[m.len() - 1].en, "Help");
            assert!(m.len() <= 12);
            for menu in m {
                assert!(menu.lines.len() <= 20);
                for l in menu.lines {
                    if let Line::Cmd { act: Act::Keys(keys), .. } = l {
                        assert!(!keys.is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn labels_switch_language() {
        let m = &for_path("brave.exe")[0];
        assert_eq!((m.title(crate::i18n::Lang::Id), m.title(crate::i18n::Lang::En), m.title(crate::i18n::Lang::Vi)), ("Berkas", "File", "Tệp"));
        assert_eq!((m.lines[0].label(crate::i18n::Lang::Id), m.lines[0].label(crate::i18n::Lang::En), m.lines[0].label(crate::i18n::Lang::Vi)), ("Tab Baru", "New Tab", "Thẻ mới"));
    }
}
