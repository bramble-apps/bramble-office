//! Logic patch names → one of our instruments plus settings (the matched-sounds idea for the
//! instrument library). Order of lookup: a few hand-voiced patches, then the patch's library
//! folder (from `patch_names`), refined by words in the name; unknown names fall back on keywords.

mod names {
    include!("patch_names.rs");
}

/// Library folder of a Logic instrument patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cat {
    SynthBass,
    SynthKeys,
    SynthPad,
    SynthSoundscape,
    SynthLead,
    SynthArp,
    SynthFx,
    SynthRhythmic,
    SynthPluck,
    SynthBrass,
    SynthBells,
    SynthMallets,
    SynthStrings,
    SynthPerc,
    ElectronicDrums,
    AcousticDrums,
    Percussion,
    AcousticPiano,
    ElectricPiano,
    Organ,
    Mellotron,
    Clavinet,
    Bass,
    Horns,
    Strings,
    Mallet,
    Guitar,
    Orchestral,
    World,
}

/// Which of our instruments plays a patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    VintageKeysEp,
    TonewheelB3,
    Clav,
    RetroSynth,
    Piano,
    DrumKit,
    Bass,
    Ensemble,
    Plucks,
}

impl Engine {
    /// Our instrument's (working) name.
    pub fn name(self) -> &'static str {
        match self {
            Engine::VintageKeysEp => crate::NAME_EP,
            Engine::TonewheelB3 => crate::NAME_B3,
            Engine::Clav => crate::NAME_CLAV,
            Engine::RetroSynth => crate::NAME_RETRO,
            Engine::Piano => crate::NAME_PIANO,
            Engine::DrumKit => crate::NAME_DRUMS,
            Engine::Bass => crate::NAME_BASS,
            Engine::Ensemble => crate::NAME_ENSEMBLE,
            Engine::Plucks => crate::NAME_PLUCKS,
        }
    }
}

/// An instrument plus parameter settings (ids as in that instrument's `params()`).
#[derive(Clone, Debug, PartialEq)]
pub struct PatchSpec {
    pub engine: Engine,
    pub settings: Vec<(u32, f32)>,
    /// The library folder the patch was found in (None = matched on keywords only).
    pub category: Option<Cat>,
}

fn spec(engine: Engine, settings: &[(u32, f32)]) -> PatchSpec {
    PatchSpec {
        engine,
        settings: settings.to_vec(),
        category: None,
    }
}

fn has(s: &str, words: &[&str]) -> bool {
    words.iter().any(|w| s.contains(w))
}

/// Logic patch name → instrument and settings.
pub fn patch(logic_patch_name: &str) -> Option<PatchSpec> {
    let name = logic_patch_name.trim().trim_end_matches(".patch");
    let low = name.to_lowercase();
    if let Some(s) = hand_voiced(&low) {
        return Some(s);
    }
    if let Ok(i) = names::NAMES.binary_search_by(|(n, _)| n.cmp(&name)) {
        let cat = names::NAMES[i].1;
        let mut s = from_cat(cat, &low);
        s.category = Some(cat);
        return Some(s);
    }
    // case-insensitive second try
    if let Some((_, cat)) = names::NAMES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
    {
        let mut s = from_cat(*cat, &low);
        s.category = Some(*cat);
        return Some(s);
    }
    keyword_cat(&low).map(|c| from_cat(c, &low))
}

/// Number of Logic patch names the table knows by folder.
pub fn known_patch_count() -> usize {
    names::NAMES.len()
}

pub fn known_patch_names() -> impl Iterator<Item = &'static str> {
    names::NAMES.iter().map(|(n, _)| *n)
}

fn hand_voiced(low: &str) -> Option<PatchSpec> {
    use Engine::*;
    Some(match low {
        "deluxe classic" => spec(VintageKeysEp, &[(0, 15.0)]),
        "deluxe modern" => spec(VintageKeysEp, &[(0, 14.0), (11, 6.0)]),
        "classic suitcase mk iv" => spec(
            VintageKeysEp,
            &[(0, 3.0), (32, 1.0), (19, 45.0), (36, 0.0), (18, 4.5)],
        ),
        "bright suitcase" => spec(
            VintageKeysEp,
            &[
                (0, 1.0),
                (32, 1.0),
                (19, 40.0),
                (36, 0.0),
                (18, 5.0),
                (11, 6.0),
            ],
        ),
        "wide suitcase" => spec(
            VintageKeysEp,
            &[
                (0, 2.0),
                (32, 1.0),
                (19, 55.0),
                (36, 0.0),
                (18, 3.5),
                (20, 180.0),
            ],
        ),
        "stage mk i flanger" => spec(VintageKeysEp, &[(0, 4.0), (33, 1.0), (21, 0.7), (25, 0.4)]),
        "wurlitzer classic" => spec(VintageKeysEp, &[(0, 16.0)]),
        "wurlitzer modern" => spec(VintageKeysEp, &[(0, 18.0)]),
        "distorted wurli" => spec(VintageKeysEp, &[(0, 17.0), (30, 1.0), (13, 5.0)]),
        "phasing bells" => spec(
            VintageKeysEp,
            &[(0, 11.0), (31, 1.0), (16, 0.6), (17, 0.5), (15, 0.4)],
        ),
        "shimmering electric piano" => spec(VintageKeysEp, &[(0, 14.0), (33, 1.0), (21, 0.6)]),
        "classic rock organ" => spec(
            TonewheelB3,
            &[
                (0, 8.0),
                (1, 8.0),
                (2, 8.0),
                (3, 8.0),
                (4, 0.0),
                (5, 0.0),
                (6, 0.0),
                (7, 0.0),
                (8, 0.0),
                (17, 0.6),
                (15, 6.0),
            ],
        ),
        "jazz organ" => spec(
            TonewheelB3,
            &[
                (0, 8.0),
                (1, 8.0),
                (2, 8.0),
                (3, 0.0),
                (4, 0.0),
                (5, 0.0),
                (6, 0.0),
                (7, 0.0),
                (8, 0.0),
                (9, 1.0),
                (10, 1.0),
                (11, 0.4),
                (15, 6.0),
                (17, 0.15),
            ],
        ),
        "steinway grand piano" | "concert grand" | "studio grand" => spec(Piano, &[(13, 0.0)]),
        _ => return None,
    })
}

/// Words that place an unknown name in a folder.
fn keyword_cat(low: &str) -> Option<Cat> {
    let c = if has(
        low,
        &["drum", "kit", "808", "909", "beat", "snare", "kick", "linn"],
    ) {
        if has(
            low,
            &[
                "808", "909", "electro", "machine", "trap", "house", "techno", "linn", "dance",
                "hip hop",
            ],
        ) {
            Cat::ElectronicDrums
        } else {
            Cat::AcousticDrums
        }
    } else if has(low, &["perc", "conga", "bongo", "shaker", "tambourine"]) {
        Cat::Percussion
    } else if has(
        low,
        &[
            "rhodes",
            "wurli",
            "e-piano",
            "electric piano",
            "epiano",
            "suitcase",
            "tine",
        ],
    ) {
        Cat::ElectricPiano
    } else if has(low, &["piano", "grand", "upright"]) && !has(low, &["bass"]) {
        Cat::AcousticPiano
    } else if has(low, &["organ", "b3", "tonewheel", "drawbar"]) {
        Cat::Organ
    } else if has(low, &["clav"]) {
        Cat::Clavinet
    } else if has(low, &["mellotron", "tape"]) {
        Cat::Mellotron
    } else if has(
        low,
        &[
            "synth bass",
            "sub bass",
            "acid",
            "808 bass",
            "analog bass",
            "reese",
            "wobble",
        ],
    ) {
        Cat::SynthBass
    } else if has(low, &["bass"]) {
        Cat::Bass
    } else if has(
        low,
        &[
            "string",
            "violin",
            "viola",
            "cello",
            "orchestra",
            "ensemble",
        ],
    ) {
        Cat::Strings
    } else if has(
        low,
        &["brass", "horn", "trumpet", "trombone", "tuba", "sax"],
    ) {
        Cat::Horns
    } else if has(low, &["choir", "vox", "voice", "vocal", "aah", "ooh"]) {
        Cat::Orchestral
    } else if has(
        low,
        &[
            "flute", "clarinet", "oboe", "bassoon", "whistle", "recorder",
        ],
    ) {
        Cat::Orchestral
    } else if has(
        low,
        &[
            "guitar", "nylon", "acoustic", "harp", "koto", "sitar", "pluck",
        ],
    ) && !has(low, &["synth"])
    {
        Cat::Guitar
    } else if has(
        low,
        &[
            "vibe",
            "marimba",
            "xylo",
            "glock",
            "celesta",
            "music box",
            "kalimba",
            "mallet",
            "chime",
        ],
    ) {
        Cat::Mallet
    } else if has(low, &["bell"]) {
        Cat::SynthBells
    } else if has(low, &["pad", "warm", "atmos", "dream", "swell"]) {
        Cat::SynthPad
    } else if has(low, &["lead", "sync", "solo", "mono"]) {
        Cat::SynthLead
    } else if has(low, &["arp", "sequence", "seq"]) {
        Cat::SynthArp
    } else if has(
        low,
        &[
            "synth", "analog", "fm", "table", "wave", "saw", "square", "poly",
        ],
    ) {
        Cat::SynthKeys
    } else {
        return None;
    };
    Some(c)
}

fn from_cat(cat: Cat, low: &str) -> PatchSpec {
    use Engine::*;
    match cat {
        Cat::AcousticPiano => {
            let model = if has(low, &["honky", "tin can", "saloon", "boogie"]) {
                4.0
            } else if has(
                low,
                &[
                    "grandma",
                    "old-time",
                    "old time",
                    "worn",
                    "capstan",
                    "record collection",
                    "silver age",
                ],
            ) {
                5.0
            } else if has(low, &["upright", "small", "learner"]) {
                2.0
            } else if has(
                low,
                &[
                    "felt",
                    "soft",
                    "dusk",
                    "lucid",
                    "below the surface",
                    "darkened",
                    "cinematic",
                    "lo-fi",
                    "scattered",
                    "flickering",
                ],
            ) {
                3.0
            } else if has(
                low,
                &[
                    "bright",
                    "pop",
                    "amplified",
                    "70s",
                    "rise above",
                    "songwriter",
                ],
            ) {
                1.0
            } else {
                0.0
            };
            let mut s = spec(Piano, &[(13, model)]);
            if has(
                low,
                &["lo-fi", "tape", "worn", "degredation", "recording", "radio"],
            ) {
                s.settings.push((1, -0.5));
            }
            if has(
                low,
                &[
                    "ambient",
                    "dream",
                    "echoes",
                    "grain",
                    "reflected",
                    "parallel",
                    "psychic",
                    "bokeh",
                    "cloud",
                ],
            ) {
                s.settings.push((7, 0.7));
            }
            s
        }
        Cat::ElectricPiano => {
            if has(low, &["toy piano"]) {
                return spec(Plucks, &[(0, 13.0)]);
            }
            if has(low, &["80s", "digital", "signature", "electromagnetic"]) {
                return retro("fm keys");
            }
            let model = if has(low, &["wurli"]) {
                if has(low, &["funk", "distorted", "ferrous", "instant hook"]) {
                    17.0
                } else if has(low, &["modern", "synthetic", "double"]) {
                    18.0
                } else if has(low, &["soft", "flea"]) {
                    19.0
                } else {
                    16.0
                }
            } else if has(low, &["suitcase"]) {
                2.0
            } else if has(low, &["stage"]) {
                4.0
            } else if has(low, &["deep bass", "bass tines"]) {
                13.0
            } else if has(low, &["bells", "metal"]) {
                11.0
            } else if has(low, &["drive", "powered", "tube"]) {
                7.0
            } else {
                15.0
            };
            let mut s = spec(VintageKeysEp, &[(0, model)]);
            if has(low, &["suitcase", "wide", "tremolo"]) {
                s.settings
                    .extend_from_slice(&[(32, 1.0), (19, 45.0), (36, 0.0), (18, 4.5)]);
            }
            if has(low, &["phas"]) {
                s.settings
                    .extend_from_slice(&[(31, 1.0), (16, 0.6), (17, 0.5)]);
            }
            if has(
                low,
                &["drive", "distorted", "feedback", "ferrous", "powered"],
            ) {
                s.settings.extend_from_slice(&[(30, 1.0), (13, 4.0)]);
            }
            if has(low, &["shimmer", "flanger", "chorus", "double", "flying"]) {
                s.settings.extend_from_slice(&[(33, 1.0), (21, 0.6)]);
            }
            s
        }
        Cat::Organ => {
            if has(low, &["melodica", "pitched vocals", "dreamy smooth vox"]) {
                return spec(
                    Ensemble,
                    &[(
                        0,
                        if has(low, &["vox", "vocals"]) {
                            7.0
                        } else {
                            14.0
                        },
                    )],
                );
            }
            if has(low, &["grand organ", "church", "cathedral"]) {
                return spec(
                    TonewheelB3,
                    &[
                        (0, 8.0),
                        (1, 6.0),
                        (2, 8.0),
                        (3, 8.0),
                        (4, 6.0),
                        (5, 8.0),
                        (6, 4.0),
                        (7, 6.0),
                        (8, 8.0),
                        (18, 0.0),
                        (15, 0.0),
                        (16, 0.1),
                    ],
                );
            }
            let bars: [f32; 9] = if has(low, &["jazz", "bebop", "side man", "oakland"]) {
                [8.0, 8.0, 8.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
            } else if has(
                low,
                &["rock", "hard", "heavy", "power", "progressive", "metal"],
            ) {
                [8.0, 8.0, 8.0, 8.0, 0.0, 0.0, 0.0, 0.0, 0.0]
            } else if has(low, &["soul", "gospel", "rhythm", "lighter"]) {
                [8.0, 8.0, 6.0, 0.0, 0.0, 0.0, 0.0, 0.0, 4.0]
            } else if has(
                low,
                &["cheap", "gamer", "fixer", "industrial", "top end", "blips"],
            ) {
                [0.0, 0.0, 8.0, 8.0, 0.0, 8.0, 0.0, 0.0, 8.0]
            } else {
                [8.0, 8.0, 8.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
            };
            let mut s = PatchSpec {
                engine: TonewheelB3,
                settings: bars
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| (i as u32, v))
                    .collect(),
                category: None,
            };
            if has(low, &["jazz", "bebop", "side man"]) {
                s.settings
                    .extend_from_slice(&[(9, 1.0), (10, 1.0), (11, 0.4)]);
            }
            if has(low, &["rock", "hard", "heavy", "power", "metal"]) {
                s.settings.extend_from_slice(&[(17, 0.7), (18, 3.0)]);
            }
            if has(low, &["reversed", "ghostly"]) {
                s.settings.push((18, 2.0));
            }
            s
        }
        Cat::Clavinet => {
            if has(low, &["koto"]) {
                return spec(Plucks, &[(0, 6.0)]);
            }
            let mut s = spec(Clav, &[]);
            if has(low, &["wah"]) {
                s.settings
                    .push((11, if has(low, &["vintage"]) { 3.0 } else { 2.0 }));
            }
            if has(low, &["phase", "different phases", "flying"]) {
                s.settings.push((15, 1.0));
            }
            if has(
                low,
                &["distort", "pawnshop", "guitar vs", "deep funk", "digging"],
            ) {
                s.settings.extend_from_slice(&[(13, 1.0), (14, 4.0)]);
            }
            if has(low, &["muted", "chilled", "smooth"]) {
                s.settings.extend_from_slice(&[(7, 0.6), (4, 1.0)]);
            }
            if has(low, &["echo", "nice dream", "novel"]) {
                s.settings.extend_from_slice(&[(18, 1.0), (19, 0.5)]);
            }
            if has(low, &["claverotor"]) {
                s.settings.push((0, 3.0));
            }
            s
        }
        Cat::Mellotron => {
            let m = if has(low, &["choir", "vox", "voice"]) {
                13.0
            } else if has(low, &["flute"]) {
                12.0
            } else {
                11.0
            };
            let mut s = spec(Ensemble, &[(0, m)]);
            if has(low, &["brass"]) {
                s.settings.push((3, 0.8));
            }
            if has(low, &["cello"]) {
                s.settings.push((3, 0.3));
            }
            s
        }
        Cat::Bass => {
            let m = if has(low, &["upright", "monstera"]) {
                3.0
            } else if has(low, &["slap", "pop"]) {
                2.0
            } else if has(low, &["rock", "growl", "crunch", "punchy", "fuzzy", "wide"]) {
                1.0
            } else if has(
                low,
                &[
                    "sixties",
                    "vintage",
                    "retro",
                    "motown",
                    "focused",
                    "dark soul",
                ],
            ) {
                6.0
            } else {
                0.0
            };
            let mut s = spec(Bass, &[(0, m)]);
            if has(low, &["fuzzy", "crunch", "growl", "distant"]) {
                s.settings.push((5, 0.6));
            }
            if has(low, &["dance", "envelope", "swirl"]) {
                return retro("bass");
            }
            s
        }
        Cat::Horns => {
            if has(low, &["sax"]) {
                return spec(Ensemble, &[(0, 10.0), (4, 1.0)]);
            }
            let solo = has(low, &["trumpet", "trombone"]);
            spec(Ensemble, &[(0, 3.0), (4, if solo { 1.0 } else { 3.0 })])
        }
        Cat::Strings => {
            if has(low, &["violins", "violas", "cellos", "double basses"]) {
                return spec(Ensemble, &[(0, 1.0)]);
            }
            spec(Ensemble, &[(0, 0.0)])
        }
        Cat::Mallet => mallet(low),
        Cat::Guitar => {
            let m = if has(
                low,
                &["classical", "nylon", "flamenco", "lute", "oud", "guitarron"],
            ) {
                1.0
            } else if has(low, &["harp"]) {
                4.0
            } else if has(low, &["hard rock", "electric", "phasing"]) {
                2.0
            } else if has(
                low,
                &[
                    "koto",
                    "guzheng",
                    "zither",
                    "sitar",
                    "pipa",
                    "balalaika",
                    "bouzouki",
                    "saz",
                    "ruan",
                    "dulcimer",
                    "santoor",
                ],
            ) {
                6.0
            } else {
                0.0
            };
            spec(Plucks, &[(0, m)])
        }
        Cat::Orchestral | Cat::World => {
            if has(low, &["kit"]) {
                return spec(DrumKit, &[(0, 0.0)]);
            }
            if has(low, &["timpani", "taiko", "surdo"]) {
                return spec(DrumKit, &[(0, 0.0), (10, -8.0), (11, 2.0)]);
            }
            if has(low, &["choir", "singers", "ensemble", "vocal", "voice"])
                && !has(low, &["string"])
            {
                return spec(
                    Ensemble,
                    &[(
                        0,
                        if has(low, &["boys", "female"]) {
                            8.0
                        } else {
                            7.0
                        },
                    )],
                );
            }
            if has(
                low,
                &[
                    "flute",
                    "piccolo",
                    "whistle",
                    "shakuhachi",
                    "dizi",
                    "xiao",
                    "bansuri",
                    "panpipe",
                    "recorder",
                ],
            ) {
                return spec(Ensemble, &[(0, 9.0)]);
            }
            if has(
                low,
                &[
                    "clarinet",
                    "oboe",
                    "bassoon",
                    "english horn",
                    "shehnai",
                    "saxophone",
                ],
            ) {
                return spec(Ensemble, &[(0, 10.0)]);
            }
            if has(low, &["accordion"]) {
                return spec(Ensemble, &[(0, 14.0)]);
            }
            if has(low, &["horn", "trumpet", "trombone", "tuba", "brass"]) {
                return spec(
                    Ensemble,
                    &[
                        (0, 3.0),
                        (
                            4,
                            if has(low, &["solo", "tuba"]) {
                                1.0
                            } else {
                                3.0
                            },
                        ),
                    ],
                );
            }
            if has(low, &["erhu"]) {
                return spec(Ensemble, &[(0, 15.0)]);
            }
            if has(low, &["string", "violin", "viola", "cello", "basses"]) {
                return spec(
                    Ensemble,
                    &[(
                        0,
                        if has(low, &["chamber", "violins", "violas", "cellos"]) {
                            1.0
                        } else {
                            0.0
                        },
                    )],
                );
            }
            if has(low, &["harpsichord"]) {
                return spec(Plucks, &[(0, 5.0)]);
            }
            if has(low, &["harp"]) {
                return spec(Plucks, &[(0, 4.0)]);
            }
            if has(low, &["steel drum"]) {
                return spec(Plucks, &[(0, 16.0)]);
            }
            if has(low, &["gamelan", "gong", "bowl", "crotales"]) {
                return spec(Plucks, &[(0, 17.0)]);
            }
            if has(low, &["afro-cuban piano"]) {
                return spec(Piano, &[(13, 1.0)]);
            }
            if has(low, &["metal drum"]) {
                return spec(Plucks, &[(0, 16.0)]);
            }
            if has(
                low,
                &[
                    "celesta",
                    "glockenspiel",
                    "xylophone",
                    "tubular",
                    "marimba",
                    "kalimba",
                    "vibra",
                ],
            ) {
                return mallet(low);
            }
            spec(
                Plucks,
                &[(
                    0,
                    if has(
                        low,
                        &[
                            "koto", "guzheng", "zither", "sitar", "pipa", "santoor", "dulcimer",
                        ],
                    ) {
                        6.0
                    } else {
                        1.0
                    },
                )],
            )
        }
        Cat::AcousticDrums => {
            let kit = if has(
                low,
                &[
                    "brush",
                    "jazz",
                    "speakeasy",
                    "bluebird",
                    "neo soul",
                    "slow jam",
                ],
            ) {
                3.0
            } else if has(low, &["heavy", "smash", "retro rock", "east bay"]) {
                2.0
            } else if has(
                low,
                &[
                    "vintage",
                    "motown",
                    "liverpool",
                    "detroit",
                    "roots",
                    "manchester",
                    "blue ridge",
                    "portland",
                ],
            ) {
                1.0
            } else {
                0.0
            };
            spec(DrumKit, &[(0, kit)])
        }
        Cat::ElectronicDrums | Cat::SynthPerc => {
            let kit = if has(
                low,
                &[
                    "808", "trap", "boom", "hip hop", "dembow", "flex", "bump", "knock",
                ],
            ) {
                4.0
            } else if has(
                low,
                &[
                    "909", "house", "techno", "festival", "dance", "club", "hacienda",
                ],
            ) {
                5.0
            } else {
                6.0
            };
            spec(DrumKit, &[(0, kit)])
        }
        Cat::Percussion => spec(DrumKit, &[(0, 0.0)]),
        Cat::SynthBass => retro(if has(low, &["sub", "808"]) {
            "sub bass"
        } else if has(low, &["acid", "squelch", "303"]) {
            "acid bass"
        } else if has(low, &["fm", "digital", "metal"]) {
            "fm bass"
        } else {
            "bass"
        }),
        Cat::SynthLead => retro(if has(low, &["sync"]) {
            "sync lead"
        } else {
            "lead"
        }),
        Cat::SynthPad | Cat::SynthStrings => retro(
            if has(low, &["table", "glass", "digital", "shimmer", "crystal"]) {
                "table pad"
            } else if has(low, &["string"]) || cat == Cat::SynthStrings {
                "strings"
            } else {
                "pad"
            },
        ),
        Cat::SynthSoundscape | Cat::SynthFx => retro("soundscape"),
        Cat::SynthArp | Cat::SynthRhythmic => retro("pluck"),
        Cat::SynthPluck => retro(if has(low, &["fm", "glass", "bell"]) {
            "fm bells"
        } else {
            "pluck"
        }),
        Cat::SynthBrass => retro("brass"),
        Cat::SynthBells | Cat::SynthMallets => retro("fm bells"),
        Cat::SynthKeys => retro(
            if has(low, &["fm", "digital", "dx", "e-piano", "electric piano"]) {
                "fm keys"
            } else if has(low, &["table", "glass"]) {
                "table pad"
            } else if has(low, &["sync"]) {
                "sync lead"
            } else {
                "keys"
            },
        ),
    }
}

fn mallet(low: &str) -> PatchSpec {
    let m = if has(low, &["vibe", "vibra"]) {
        8.0
    } else if has(low, &["marimba"]) {
        9.0
    } else if has(low, &["xylo"]) {
        10.0
    } else if has(low, &["glock", "crotales"]) {
        11.0
    } else if has(low, &["celesta", "fairy"]) {
        12.0
    } else if has(low, &["music box", "lullaby"]) {
        13.0
    } else if has(low, &["tubular", "chime", "doorbell", "bells"]) {
        14.0
    } else if has(low, &["kalimba"]) {
        15.0
    } else {
        9.0
    };
    spec(Engine::Plucks, &[(0, m)])
}

/// Retro Synth voicings for the synth folders (parameter ids as in `retro::PARAMS`).
pub fn retro(kind: &str) -> PatchSpec {
    let s: &[(u32, f32)] = match kind {
        "bass" => &[
            (28, 1.0),
            (30, 2.0),
            (32, -12.0),
            (26, 0.4),
            (27, 0.3),
            (36, 0.32),
            (37, 0.3),
            (40, 0.5),
            (57, 0.35),
            (58, 0.25),
            (61, 0.002),
            (62, 1.0),
            (63, 0.8),
            (64, 0.08),
            (42, 0.0),
            (5, 2.0),
            (24, 0.05),
        ],
        "sub bass" => &[
            (28, 0.0),
            (30, 0.0),
            (26, 0.0),
            (27, 1.0),
            (36, 0.3),
            (37, 0.0),
            (40, 0.1),
            (61, 0.002),
            (62, 2.0),
            (63, 0.9),
            (64, 0.1),
            (42, 0.0),
            (5, 2.0),
        ],
        "acid bass" => &[
            (28, 1.0),
            (26, 0.0),
            (35, 1.0),
            (36, 0.25),
            (37, 0.8),
            (40, 0.6),
            (57, 0.2),
            (58, 0.0),
            (61, 0.001),
            (63, 1.0),
            (64, 0.05),
            (42, 0.0),
            (5, 2.0),
            (24, 0.1),
            (41, 0.4),
        ],
        "fm bass" => &[
            (19, 3.0),
            (28, 0.0),
            (30, 0.0),
            (26, 0.45),
            (25, 0.3),
            (61, 0.002),
            (62, 0.6),
            (63, 0.5),
            (64, 0.1),
            (34, 0.0),
            (42, 0.0),
        ],
        "lead" => &[
            (28, 1.0),
            (30, 1.0),
            (33, 9.0),
            (36, 0.62),
            (37, 0.25),
            (40, 0.25),
            (61, 0.005),
            (63, 0.9),
            (64, 0.25),
            (5, 2.0),
            (24, 0.08),
            (21, 1.0),
            (43, 0.0),
            (44, 0.3),
            (20, 0.0),
        ],
        "sync lead" => &[
            (19, 1.0),
            (28, 1.0),
            (30, 1.5),
            (26, 0.8),
            (25, 0.4),
            (46, 1.0),
            (48, 0.4),
            (36, 0.7),
            (37, 0.15),
            (40, 0.2),
            (61, 0.003),
            (63, 0.9),
            (64, 0.25),
            (5, 2.0),
            (24, 0.05),
            (21, 1.0),
            (43, 0.0),
        ],
        "pad" => &[
            (1, 3.0),
            (9, 0.5),
            (28, 1.0),
            (30, 2.0),
            (26, 0.5),
            (36, 0.45),
            (37, 0.15),
            (40, 0.25),
            (56, 1.2),
            (57, 2.0),
            (58, 0.6),
            (61, 0.8),
            (62, 3.0),
            (63, 0.9),
            (64, 1.6),
            (43, 0.0),
            (44, 0.6),
        ],
        "table pad" => &[
            (19, 2.0),
            (1, 2.0),
            (9, 0.4),
            (28, 2.4),
            (30, 2.8),
            (25, 0.3),
            (46, 0.0),
            (48, 0.2),
            (36, 0.6),
            (61, 0.6),
            (62, 3.0),
            (63, 0.8),
            (64, 1.8),
            (43, 0.0),
            (44, 0.5),
        ],
        "strings" => &[
            (1, 4.0),
            (9, 0.55),
            (28, 1.0),
            (30, 1.0),
            (36, 0.55),
            (37, 0.1),
            (40, 0.1),
            (61, 0.35),
            (63, 0.95),
            (64, 0.9),
            (43, 0.0),
            (44, 0.7),
        ],
        "soundscape" => &[
            (19, 2.0),
            (1, 3.0),
            (9, 0.6),
            (28, 3.0),
            (30, 1.5),
            (25, 0.7),
            (46, 6.0),
            (48, 0.15),
            (39, 0.3),
            (36, 0.5),
            (37, 0.4),
            (61, 1.5),
            (62, 4.0),
            (63, 0.7),
            (64, 3.0),
            (43, 1.0),
            (44, 0.6),
            (45, 0.1),
        ],
        "pluck" => &[
            (28, 1.0),
            (30, 2.0),
            (33, 6.0),
            (36, 0.35),
            (37, 0.35),
            (40, 0.6),
            (56, 0.0),
            (57, 0.25),
            (58, 0.0),
            (61, 0.001),
            (62, 0.5),
            (63, 0.0),
            (64, 0.3),
            (43, 0.0),
            (44, 0.4),
        ],
        "brass" => &[
            (1, 2.0),
            (9, 0.3),
            (28, 1.0),
            (30, 1.0),
            (33, 6.0),
            (36, 0.35),
            (37, 0.1),
            (40, 0.55),
            (56, 0.06),
            (57, 0.5),
            (58, 0.45),
            (61, 0.03),
            (63, 0.85),
            (64, 0.25),
            (43, 0.0),
            (44, 0.3),
        ],
        "fm bells" => &[
            (19, 3.0),
            (28, 0.0),
            (30, 2.0),
            (32, 2.0),
            (26, 0.35),
            (34, 0.0),
            (61, 0.001),
            (62, 3.0),
            (63, 0.0),
            (64, 2.0),
            (43, 0.0),
            (44, 0.4),
            (65, 0.6),
        ],
        "fm keys" => &[
            (19, 3.0),
            (28, 0.0),
            (30, 0.0),
            (32, 12.0),
            (26, 0.25),
            (34, 0.0),
            (61, 0.001),
            (62, 2.0),
            (63, 0.2),
            (64, 0.4),
            (43, 0.0),
            (44, 0.4),
            (65, 0.7),
        ],
        _ => &[
            (28, 1.0),
            (30, 2.0),
            (36, 0.5),
            (40, 0.3),
            (57, 1.0),
            (58, 0.4),
            (61, 0.003),
            (62, 1.5),
            (63, 0.6),
            (64, 0.3),
        ],
    };
    spec(Engine::RetroSynth, s)
}
