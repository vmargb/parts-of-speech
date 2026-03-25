use eframe::egui::Color32;

// -- Theme types ---------------------------------------------------------------

#[derive(Clone, PartialEq, Debug)]
pub enum ThemeKind {
    // Dark themes
    Dark,
    Peach,
    ColdBlue,
    Forest,
    Midnight,
    // Light themes
    Light,
    Blossom,
    Glacier,
    Meadow,
    Dusk,
}

impl ThemeKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Dark     => "DARK",
            Self::Peach    => "PEACH",
            Self::ColdBlue => "ARCTIC",
            Self::Forest   => "FOREST",
            Self::Midnight => "MIDNIGHT",
            Self::Light    => "LIGHT",
            Self::Blossom  => "BLOSSOM",
            Self::Glacier  => "GLACIER",
            Self::Meadow   => "MEADOW",
            Self::Dusk     => "DUSK",
        }
    }

    pub fn all() -> &'static [ThemeKind] {
        &[
            ThemeKind::Dark, ThemeKind::Peach, ThemeKind::ColdBlue,
            ThemeKind::Forest, ThemeKind::Midnight,
            ThemeKind::Light, ThemeKind::Blossom, ThemeKind::Glacier,
            ThemeKind::Meadow, ThemeKind::Dusk,
        ]
    }

    pub fn dark_themes() -> &'static [ThemeKind] {
        &[
            ThemeKind::Dark, ThemeKind::Peach, ThemeKind::ColdBlue,
            ThemeKind::Forest, ThemeKind::Midnight,
        ]
    }

    pub fn light_themes() -> &'static [ThemeKind] {
        &[
            ThemeKind::Light, ThemeKind::Blossom, ThemeKind::Glacier,
            ThemeKind::Meadow, ThemeKind::Dusk,
        ]
    }
}

pub struct Palette {
    pub bg:     Color32,
    pub surf:   Color32,
    pub surf2:  Color32,
    pub surf3:  Color32,
    pub border: Color32,
    pub bordbr: Color32,
    pub rec:    Color32,
    pub play:   Color32,
    pub amber:  Color32,
    pub blue:   Color32,
    pub muted:  Color32,
    pub text:   Color32,
    pub dim:    Color32,
    pub mono:   Color32,
}

pub fn palette_for(theme: &ThemeKind) -> Palette {
    match theme {

        // -- Dark themes --------------------------------------------------------

        ThemeKind::Dark => Palette {
            bg:     Color32::from_rgb(11,  11,  15 ),
            surf:   Color32::from_rgb(18,  18,  24 ),
            surf2:  Color32::from_rgb(24,  24,  34 ),
            surf3:  Color32::from_rgb(32,  32,  46 ),
            border: Color32::from_rgb(40,  40,  58 ),
            bordbr: Color32::from_rgb(60,  60,  84 ),
            rec:    Color32::from_rgb(229, 72,  77 ),
            play:   Color32::from_rgb(46,  204, 143),
            amber:  Color32::from_rgb(245, 166, 35 ),
            blue:   Color32::from_rgb(74,  144, 217),
            muted:  Color32::from_rgb(72,  72,  100),
            text:   Color32::from_rgb(237, 236, 233),
            dim:    Color32::from_rgb(100, 98,  120),
            mono:   Color32::from_rgb(148, 226, 199),
        },
        ThemeKind::Peach => Palette {
            bg:     Color32::from_rgb(15,  10,  8  ),
            surf:   Color32::from_rgb(25,  17,  13 ),
            surf2:  Color32::from_rgb(36,  24,  18 ),
            surf3:  Color32::from_rgb(48,  32,  24 ),
            border: Color32::from_rgb(70,  46,  32 ),
            bordbr: Color32::from_rgb(100, 68,  48 ),
            rec:    Color32::from_rgb(218, 82,  64 ),
            play:   Color32::from_rgb(200, 152, 76 ),
            amber:  Color32::from_rgb(238, 172, 86 ),
            blue:   Color32::from_rgb(108, 156, 210),
            muted:  Color32::from_rgb(105, 72,  55 ),
            text:   Color32::from_rgb(248, 234, 218),
            dim:    Color32::from_rgb(148, 110, 86 ),
            mono:   Color32::from_rgb(228, 192, 152),
        },
        ThemeKind::ColdBlue => Palette {
            bg:     Color32::from_rgb(8,   12,  20 ),
            surf:   Color32::from_rgb(11,  18,  32 ),
            surf2:  Color32::from_rgb(15,  25,  46 ),
            surf3:  Color32::from_rgb(20,  34,  62 ),
            border: Color32::from_rgb(28,  46,  86 ),
            bordbr: Color32::from_rgb(46,  74,  132),
            rec:    Color32::from_rgb(215, 76,  96 ),
            play:   Color32::from_rgb(54,  198, 198),
            amber:  Color32::from_rgb(96,  178, 238),
            blue:   Color32::from_rgb(76,  158, 255),
            muted:  Color32::from_rgb(48,  76,  132),
            text:   Color32::from_rgb(208, 224, 248),
            dim:    Color32::from_rgb(78,  108, 164),
            mono:   Color32::from_rgb(118, 208, 230),
        },
        ThemeKind::Forest => Palette {
            bg:     Color32::from_rgb(8,   13,  10 ),
            surf:   Color32::from_rgb(12,  20,  14 ),
            surf2:  Color32::from_rgb(16,  28,  18 ),
            surf3:  Color32::from_rgb(22,  38,  25 ),
            border: Color32::from_rgb(30,  54,  33 ),
            bordbr: Color32::from_rgb(46,  82,  50 ),
            rec:    Color32::from_rgb(208, 78,  78 ),
            play:   Color32::from_rgb(74,  198, 116),
            amber:  Color32::from_rgb(198, 162, 58 ),
            blue:   Color32::from_rgb(78,  158, 198),
            muted:  Color32::from_rgb(52,  92,  60 ),
            text:   Color32::from_rgb(212, 240, 218),
            dim:    Color32::from_rgb(88,  132, 94 ),
            mono:   Color32::from_rgb(128, 208, 146),
        },
        ThemeKind::Midnight => Palette {
            bg:     Color32::from_rgb(10,  8,   18 ),
            surf:   Color32::from_rgb(15,  12,  30 ),
            surf2:  Color32::from_rgb(21,  16,  44 ),
            surf3:  Color32::from_rgb(29,  22,  58 ),
            border: Color32::from_rgb(46,  34,  84 ),
            bordbr: Color32::from_rgb(70,  52,  126),
            rec:    Color32::from_rgb(218, 68,  178),
            play:   Color32::from_rgb(118, 98,  238),
            amber:  Color32::from_rgb(178, 138, 255),
            blue:   Color32::from_rgb(98,  158, 255),
            muted:  Color32::from_rgb(78,  58,  118),
            text:   Color32::from_rgb(228, 218, 248),
            dim:    Color32::from_rgb(118, 98,  158),
            mono:   Color32::from_rgb(158, 138, 255),
        },

        // -- Light themes -------------------------------------------------------
        // Light: clean neutral — mirrors Dark

        ThemeKind::Light => Palette {
            bg:     Color32::from_rgb(238, 238, 245),
            surf:   Color32::from_rgb(226, 226, 236),
            surf2:  Color32::from_rgb(212, 212, 224),
            surf3:  Color32::from_rgb(196, 196, 212),
            border: Color32::from_rgb(178, 178, 200),
            bordbr: Color32::from_rgb(140, 140, 170),
            rec:    Color32::from_rgb(196, 48,  54 ),
            play:   Color32::from_rgb(28,  158, 108),
            amber:  Color32::from_rgb(175, 118, 18 ),
            blue:   Color32::from_rgb(50,  108, 188),
            muted:  Color32::from_rgb(155, 155, 185),
            text:   Color32::from_rgb(28,  28,  48 ),
            dim:    Color32::from_rgb(100, 98,  128),
            mono:   Color32::from_rgb(28,  152, 122),
        },

        // Blossom: warm cream — mirrors Peach

        ThemeKind::Blossom => Palette {
            bg:     Color32::from_rgb(255, 248, 242),
            surf:   Color32::from_rgb(244, 232, 220),
            surf2:  Color32::from_rgb(232, 214, 198),
            surf3:  Color32::from_rgb(218, 194, 172),
            border: Color32::from_rgb(198, 168, 144),
            bordbr: Color32::from_rgb(165, 130, 105),
            rec:    Color32::from_rgb(185, 62,  42 ),
            play:   Color32::from_rgb(162, 112, 38 ),
            amber:  Color32::from_rgb(190, 128, 28 ),
            blue:   Color32::from_rgb(68,  118, 182),
            muted:  Color32::from_rgb(188, 158, 135),
            text:   Color32::from_rgb(58,  32,  18 ),
            dim:    Color32::from_rgb(138, 102, 80 ),
            mono:   Color32::from_rgb(168, 128, 72 ),
        },

        // Glacier: pale sky — mirrors Arctic/ColdBlue

        ThemeKind::Glacier => Palette {
            bg:     Color32::from_rgb(238, 245, 255),
            surf:   Color32::from_rgb(222, 234, 252),
            surf2:  Color32::from_rgb(205, 222, 248),
            surf3:  Color32::from_rgb(186, 208, 240),
            border: Color32::from_rgb(165, 192, 228),
            bordbr: Color32::from_rgb(122, 158, 210),
            rec:    Color32::from_rgb(182, 48,  68 ),
            play:   Color32::from_rgb(28,  150, 158),
            amber:  Color32::from_rgb(42,  128, 195),
            blue:   Color32::from_rgb(36,  105, 205),
            muted:  Color32::from_rgb(148, 172, 215),
            text:   Color32::from_rgb(14,  28,  62 ),
            dim:    Color32::from_rgb(68,  98,  155),
            mono:   Color32::from_rgb(25,  148, 172),
        },

        // Meadow: pale sage — mirrors Forest

        ThemeKind::Meadow => Palette {
            bg:     Color32::from_rgb(238, 248, 238),
            surf:   Color32::from_rgb(222, 238, 224),
            surf2:  Color32::from_rgb(205, 226, 208),
            surf3:  Color32::from_rgb(186, 212, 190),
            border: Color32::from_rgb(162, 194, 167),
            bordbr: Color32::from_rgb(118, 160, 125),
            rec:    Color32::from_rgb(178, 52,  52 ),
            play:   Color32::from_rgb(38,  148, 72 ),
            amber:  Color32::from_rgb(148, 118, 22 ),
            blue:   Color32::from_rgb(38,  118, 162),
            muted:  Color32::from_rgb(148, 182, 155),
            text:   Color32::from_rgb(18,  48,  22 ),
            dim:    Color32::from_rgb(72,  118, 78 ),
            mono:   Color32::from_rgb(32,  148, 70 ),
        },

        // Dusk: pale lavender — mirrors Midnight

        ThemeKind::Dusk => Palette {
            bg:     Color32::from_rgb(245, 240, 255),
            surf:   Color32::from_rgb(232, 224, 252),
            surf2:  Color32::from_rgb(218, 208, 246),
            surf3:  Color32::from_rgb(200, 188, 236),
            border: Color32::from_rgb(178, 162, 222),
            bordbr: Color32::from_rgb(142, 120, 195),
            rec:    Color32::from_rgb(182, 38,  152),
            play:   Color32::from_rgb(85,  58,  195),
            amber:  Color32::from_rgb(138, 95,  218),
            blue:   Color32::from_rgb(55,  108, 210),
            muted:  Color32::from_rgb(172, 158, 212),
            text:   Color32::from_rgb(28,  14,  62 ),
            dim:    Color32::from_rgb(98,  75,  148),
            mono:   Color32::from_rgb(95,  75,  205),
        },
    }
}
