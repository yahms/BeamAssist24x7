//! "What can I ask?" — command guide, overall or per category.

pub struct Category {
    pub key: &'static str,
    pub title: &'static str,
    pub examples: &'static [&'static str],
}

pub const CATEGORIES: &[Category] = &[
    Category {
        key: "vehicle",
        title: "Vehicle",
        examples: &[
            "high beams / low beams / headlights off",
            "fog lights on · hazards on · indicate left · cancel indicator",
            "lock the rear diff · unlock all diffs · centre diff 50%",
            "low range · four wheel drive · 2wd",
            "traction control off",
            "park · drive · reverse · neutral · third gear",
            "temperatures · fuel · engine · speed · damage",
            "start the engine · stop the engine",
            "open the doors · open the driver door · close the rear left door · open the bonnet · open the boot",
            "jato on / off · arm the nitrous · disarm nos",
            "what can I switch",
            "make the white sunburst black · paint my car fire red",
            "repair (fix, back on the road) · respawn (back to start)",
        ],
    },
    Category {
        key: "truck",
        title: "Trucks",
        examples: &[
            "engine brake on · engine brake level 2 · jake brake off",
            "retarder auto · retarder 60% · retarder off",
            "parking brake on · release the handbrake",
            "raise the suspension · lower the suspension · stop the suspension",
        ],
    },
    Category {
        key: "nav",
        title: "Satnav & places",
        examples: &[
            "list places · list gas stations",
            "navigate to chinatown · set satnav to the drag strip · show me how to get to the quarry",
            "where is the nearest charging station · find the nearest gas station",
            "how far is the nearest service station · what direction is the race track",
            "cancel satnav · clear the route",
            "autopilot on (drives the satnav route) · autopilot off",
            "take me to the dockyards · drive me to the nearest gas station",
            "teleport to the highway · spawn at the race track",
        ],
    },
    Category {
        key: "spawn",
        title: "Cars & spawning",
        examples: &[
            "spawn bruckell legran",
            "spawn legran luxe v6 manual",
            "replace car with a pigeon · add a percheron",
            "spawn an ai van (you stay in your car)",
            "list legran configs · list cars",
        ],
    },
    Category {
        key: "map",
        title: "Maps",
        examples: &["list maps", "what map is this", "change map to west coast"],
    },
    Category {
        key: "env",
        title: "Weather & time",
        examples: &[
            "set the time to 6 pm · half past 7 · sunset · noon",
            "stop time · let time run · day length 10 minutes",
            "clear skies · cloudy · overcast",
            "light fog · heavy fog · no fog",
            "wind 20 km/h · calm",
            "light rain · heavy rain · stormy · no rain (maps with rain)",
            "moon gravity · zero gravity · normal gravity",
            "what weather settings can I change",
        ],
    },
    Category {
        key: "ai",
        title: "Traffic & AI",
        examples: &[
            "spawn traffic · add 5 traffic cars · remove traffic",
            "spawn 2 police cars · police chase me",
            "make the white van chase me · tell the blue pickup to follow me",
            "stop the red taxi · make the cops run away",
            "make all traffic drive aggressively · limit the taxi to 50 km/h",
            "autopilot on / off",
        ],
    },
    Category {
        key: "game",
        title: "Game",
        examples: &["pause the game · unpause · close menu · back to the game", "slow motion · half speed · real time", "what map is this"],
    },
];

/// Map a spoken category word to a category key.
pub fn category_of(word: &str) -> Option<&'static str> {
    Some(match word {
        "vehicle" | "driving" | "drivetrain" | "light" | "lights" | "diff" | "gear" | "gears" | "paint" | "control" => "vehicle",
        "spawn" | "spawning" | "cars" | "car" | "config" | "configs" => "spawn",
        "map" | "maps" | "level" | "levels" => "map",
        "weather" | "time" | "environment" | "rain" | "fog" | "gravity" => "env",
        "traffic" | "ai" | "police" | "cop" | "cops" => "ai",
        "game" | "menu" | "ui" | "pause" => "game",
        "truck" | "trucks" | "brake" | "brakes" | "retarder" | "suspension" => "truck",
        "satnav" | "nav" | "navigation" | "gps" | "places" | "locations" | "destinations" | "autopilot" | "teleport" => "nav",
        _ => return None,
    })
}

/// (label, in-game text, console text)
pub fn guide(cat: Option<&str>) -> (String, String, String) {
    match cat.and_then(|k| CATEGORIES.iter().find(|c| c.key == k)) {
        Some(c) => (
            format!("{} commands", c.title),
            c.examples.join("; "),
            c.examples.iter().map(|e| format!("  {}", e)).collect::<Vec<_>>().join("\n"),
        ),
        None => {
            let titles: Vec<&str> = CATEGORIES.iter().map(|c| c.title).collect();
            let text = format!(
                "I can help with: {}. Ask e.g. 'what vehicle commands are there' or 'help with weather'",
                titles.join(", ")
            );
            let mut console = String::new();
            for c in CATEGORIES {
                console.push_str(&format!("{}:\n", c.title));
                for e in c.examples {
                    console.push_str(&format!("  {}\n", e));
                }
            }
            ("Help".into(), text, console)
        }
    }
}
