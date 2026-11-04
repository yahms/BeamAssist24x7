# BeamAssist24x7

*Voice control for BeamNG.Drive. Ask for anything you can think of. (results may vary)*

## Roadside assistance, upgraded to Gold

Every copy of BeamNG.drive ships with **Standard** roadside cover. You press a key and your car is "recovered" onto a road. You may never have asked which road, or how far above it.

**BeamAssist24x7 Gold** is the premium tier for drivers who are tired of using their hands. Just speak. A dispatcher works out what you meant (mostly), then carries it out in the game straight away. A short message confirms what was done.

| | Standard (included) | **Gold (BeamAssist24x7)** |
|---|---|---|
| Recovery | Press a key | Say "repair the car" |
| Respawn | Press a different key | Say "respawn", and fall 15 m with dignity |
| Diff locks | Find the right menu for this particular truck | "Lock the rear diff": every rear diff, the inter-axle diff too, and any you didn't know about |
| Weather | Settings menu | "Make it rain", on maps that have rain |
| Traffic | Radial menu | "Spawn two police cars", then "make the white van chase me" |
| Paint shop | Vehicle config, colour tab | "Make the white Sunburst black" |
| Call centre | None | None, but the dispatcher listens 24/7 |

Gold members are greeted by name: "hey beam" or "computer". Waiting times are measured in milliseconds, unless you have just loaded West Coast USA.

### What our members say

> "I crashed my LeGran and asked for a Percheron. I can't see my LeGran any more now that it's under the Percheron. Couldn't be happier."
> — **Bruce, East Coast USA** ★★★★★

> "I rolled my Ibishu Pigeon onto its roof near the top of Utah and called BeamAssist. They set the time to 6 pm. I'm still upside down, but honestly the sunset was gorgeous."
> — **Darren, Utah** ★★★★★

> "I asked them to make it rain. They didn't send money, they sent weather. I drive a convertible with the roof off. I have never felt so alive."
> — **Priya, West Coast USA** ★★★★★

> "I asked them to unlock my rear diff. They sent four police cars. The diff is still locked, but nobody is messing with me now."
> — **Kaz, Gridmap** ★★★★★

> "I was being chased and asked them to make the police back off. They set every cruiser to 'flee'. Now I chase them. I've found my calling."
> — **Rhys, Italy** ★★★★★

> "I said 'take me home' and they respawned me. Home turns out to be 15 metres above the start line. I'd never seen my neighbourhood from up there. Lovely spot, briefly."
> — **Moana, Jungle Rock Island** ★★★★☆

> "My D-Series was stuck in a ditch. The technician set gravity to the Moon's. The truck is out of the ditch. It is also leaving the atmosphere."
> — **Hank, Johnson Valley** ★★★★★

> "I asked them to make the traffic calm down. Every driver on the map is now set to 'calm'. My commute takes four hours and I've finished three audiobooks. Life-changing."
> — **Sandra, Automation Test Track** ★★★★★

> "I said 'computer' on a work call and my Sunburst went into low range. Fastest response I've ever had from anyone."
> — **Glen, Small Island** ★★★★★

### Terms of cover

BeamAssist24x7 Gold accepts no liability for loss of your no-claims bonus, write-offs, depreciation, whiplash, deformed chassis rails, or any vehicle reduced to a single cubic metre of soft-body. **Occupants are not modelled, and so are not covered.** Next of kin will be told by in-game message, shown for 5 seconds. Your vehicle's resale value has always been zero.

By saying any wake phrase within range of a microphone, you agree to automatic renewal at 29.99 a month, billed in node-beams. You may cancel only in writing, by fax, to a dealership on a map you have not installed. Towing distance is unlimited; towing direction is "up". Seatbelts are decorative. The Platinum tier does exist, and you can't afford it.

### Platinum: by invitation only

You don't buy Platinum. Platinum buys you.

Joining takes one word: "yes", said at the end of a long drive while a pre-ticked box flashes on screen for one frame. The single clause of the terms transfers you, your future vehicles and your voice to BeamAssist24x7. Your voice becomes a wake phrase.

- **Pre-emptive recovery.** Your car is repaired a few seconds *before* the crash.
- **Rewind on demand.** Say "that didn't happen" and it didn't.
- **One-word navigation.** Say "home". We move home to you.
- **Guaranteed respawn.** On the ground, upright, facing the right way. Our engineers call this the most impossible feature on the list.

Platinum costs exactly what you have, and who you are. Membership can't be cancelled, only outlived. Members who try to leave are recovered to a nearby road.

*BeamAssist24x7 is a fan project, not affiliated with BeamNG GmbH. No real cars, wallets or occupants were harmed.*

---

## The technical bit

The voice assistant is `beam-assist.exe` (formerly BeamNG.Speak). It talks to the game's built-in MCP server and runs every model locally.

**Status: 0.2.0. Milestone 2, stage A: voice input.**
- You can speak or type commands, and the exe carries them out in the game.
- Speech is recognised locally with Whisper and goes through the same command parser as typed text.
- Stage B adds the embedding router for free phrasing; stage C adds the small LLM for tie-breaks.

### Roadmap

**0.2.0** (on the `dev` branch; not yet tested in the game)
- [x] **Map loading that survives the freeze.** Load through the game's own freeroam start, so the full set of game extensions is loaded. Show "loading…", wait out the 15–20 s when the MCP stops answering, then close any menu.
- [x] **Traffic and police fixes.** Load the missing spawn extension before spawning, and make "spawn ai" and "spawn ai cars" add traffic.
- [x] **Pause.** "pause the game" and "unpause" (the in-game J key).
- [x] **Satnav.**
  - "navigate to", "set satnav to", "show me how to get to" and "where is X" draw the route to a named place: spawn points, fuel and charging stations, drag strips.
  - "clear" or "cancel satnav" removes it.
- [x] **Distances.** "How far is the nearest fuel station?" and "What's the distance to X?" answer in m or km, with a compass direction (N, NE, E…).
- [x] **Autopilot.** "autopilot on" drives the route currently in the satnav, and "autopilot off" hands back control. "drive me to" and "take me to" set the route and drive it.
- [x] **Instant moves.** "spawn at" and "teleport to" a named place.
- [x] **Body controls.** Open, close or toggle each door (front/rear, left/right, or all), the bonnet/hood and the boot/trunk/tailgate.
- [x] **Engine start and stop** in one command. The starter is held until the engine catches.
- [x] **Boost.** JATO on or off, and arm or disarm nitrous.
- [x] **Truck controls.**
  - Engine (compression) brake: off or level 1–3.
  - Retarder: auto, off, or 20–100%.
  - Parking brake.
  - Air suspension: raise, lower or stop.
- [x] **Longer messages for answers.** A separate on-screen time for satnav information and replies to questions (`ttl_info`).

**Then, in this order**
- [ ] **Stage B: embedding router.** A small embedding model (bge-small, run with Candle) maps free phrasing onto the same commands, so wording doesn't have to match the built-in phrases.
- [ ] **Stage C: tiny LLM tie-breaks.** Qwen3 0.6B (Candle) only picks between 2–3 candidate commands and fills in slots such as names and numbers. It never writes replies.
- [ ] **Optional stages B and C.** Switch the router and the LLM on or off separately in `config.toml`. With both off, only the rule-based parser runs, which saves roughly 0.6 GB of RAM or VRAM and skips downloading those models.
- [ ] **GPU for the router and LLM,** matching the speech recognition's GPU-or-CPU choice.
- [ ] **Chained commands.** Several requests in one sentence, such as "load the west coast map and put me in a Bastion" or "spawn a Pigeon, then drive to the nearest gas station".
  - **Splitting:** the request is split on words such as "then", "and then", "after that" and "once", and on "and" or commas when each side is a whole command. An "and" inside a single command stays put ("lock the front and rear diffs").
  - **Order:** steps run in the order spoken, except where the game needs a different order. A map load always goes first, because it resets the vehicle, then spawning, then vehicle settings, then driving.
  - **Waiting:** each step waits for the previous one to finish and be confirmed: the map loaded, the car spawned, the door shut. Spoken timing works too: "in 10 seconds", "after 30 seconds", "when I get there" (when the route ends).
  - **Waits don't block.** A waiting chain is parked, and other commands still run in the meantime: you can change the lights or open a door while you wait for "in 30 seconds". During a map load the game itself doesn't answer, so commands given then are held and run once the map is ready. A chain keeps hold of the vehicle it spawned, so "it" still means that car even if you spawn another one in between.
  - **Warning before timed steps:** 10 seconds before a timed step runs (`warn_before`, configurable 5–10 s), a message shows what is about to happen, for example "Drive to Nodeoline Gas Station in 10 s; say 'cancel' to stop". Waits shorter than that warn straight away.
  - **References:** "it" or "the car" points to the vehicle from the step before ("spawn a Pigeon and paint it red").
  - **Results:** a chain stops at the first step that fails, and one message lists every step with its result. "cancel" stops a chain that is still running.
- [ ] **HTTP API and phone microphone page.** Lets other devices on the network control BeamAssist. Off by default; when on, it uses one port and needs a token.
  - **Commands:** `POST /api/command` takes free text through the normal parser. `POST /api/intent` takes structured commands such as `{"action":"lights","state":"high"}`. Either way you get back `{intent, label, result, status}`, and the in-game message still appears.
  - **Game state:** `GET /api/state` returns paused / in game / menu, the map and the vehicle. `GET /api/events` (SSE) pushes state changes and command results as they happen.
  - **State is live but rate-limited.** A state query triggers a live read from the game, at most one every 500 ms. Between reads, BeamAssist answers straight away from its last read. Add `live=true` to wait for the next read instead (up to 500 ms), for when accuracy matters more than speed. Commands never use this: they always read the vehicle at the moment they run.
  - **Phone microphone page:** served by the exe itself. It uses HTTPS with a self-signed certificate created on first run, because phone browsers only allow the microphone over HTTPS. The console shows a QR code to pair a phone.
    - It offers hold-to-talk, plus an open-mic mode that uses the normal wake phrases.
    - Phone audio goes through the same speech recognition as the PC microphone.
    - Basic display only: whether the game is connected, the game state (paused, in game or menu) and the map, plus a running log of each command sent and the game's reply.

**Later**
- [ ] **Web UI.** A full control page served from the same API: vehicle controls, satnav, traffic and environment buttons, and live readouts.
- [ ] **MQTT bridge.** Connects to your own broker: commands in on `beamassist/cmd` and `beamassist/intent/<action>`, results on `beamassist/result`, and retained state topics. For Home Assistant, Node-RED and ESP32/Arduino button boxes.
- [ ] **Stream Deck plugin** with keys that light up from live state, and an example ESP32 button box.
- [ ] **Facilities as destinations.** Garages, dealerships, and delivery points such as the hospital and police station.
- [ ] **"Driver" and "passenger" doors** that follow left- or right-hand drive.
- [ ] **More truck controls:** hydraulics idle raise, and air pressure readouts.
- [ ] **Windows UI and tray icon.** A small window for status, the last few commands and settings, with an option to sit in the system tray instead of a console. In the tray it waits for the game to start, then connects on its own.
- [ ] **Linux version.** A native Linux build, for the game running under Proton or for a separate machine on the LAN: ALSA/PulseAudio/PipeWire microphone input through cpal, and Vulkan for speech recognition on the GPU. The Linux build won't check free GPU memory before loading a model; it uses the GPU if one is found, and you can force the CPU in `config.toml`.

### Build (Windows)

You need:

1. Rust with the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`).
2. The Visual Studio C++ Build Tools ("Desktop development with C++").
3. **CMake** (whisper.cpp is compiled from source).
4. **LLVM/clang** (whisper-rs generates its C bindings with libclang).

Install CMake and LLVM, then open a **new** terminal so PATH updates:

```powershell
winget install Kitware.CMake
winget install LLVM.LLVM
```

`build.cmd` sets `LIBCLANG_PATH` to `C:\Program Files\LLVM\bin` if it isn't already set. The first build compiles whisper.cpp and takes a few minutes.

**Optional: GPU speech recognition (NVIDIA, AMD or Intel).** Install the Vulkan SDK, then open a new terminal:

```powershell
winget install KhronosGroup.VulkanSDK
```

When `VULKAN_SDK` is set, `build.cmd` builds with `--features gpu`. One exe then supports any GPU through the driver's own `vulkan-1.dll`, so no extra files sit beside the exe.

```powershell
cd <your clone folder>
.\build.cmd
```

`build.cmd` sets `RUSTFLAGS=-C target-feature=+crt-static`, so the C runtime is linked statically and the exe needs no VC++ redistributable. It then copies `beam-assist.exe` beside the script. A plain `cargo build --release` also works, but links the C runtime dynamically.

### Run

1. Start BeamNG.drive with its MCP server enabled (default `127.0.0.1:29292`).
2. Run `beam-assist.exe`. On first run it writes `config.toml` beside itself. Edit the `[mcp]` host/port there to reach the game over your LAN proxy.
3. On first start with voice enabled, it downloads the speech model (about 148 MB) and the VAD model (about 1 MB) into `models\`, and prints their full paths.
4. Speak: **"hey beam, high beams"**, or say "hey beam", pause, then the command within 6 seconds.
5. You can also type commands at the `>` prompt, or run a single command and exit: `beam-assist.exe lock rear diff`.

#### Voice settings (`[voice]` in config.toml)

| Setting | Default | Meaning |
|---|---|---|
| enabled | true | false = text only |
| mode | hotword | `hotword`: a wake phrase is required. `both`: a wake phrase is optional for short commands the parser recognises. `open`: no wake phrase |
| wake_phrases | ["hey beam", "computer"] | Matched fuzzily, because Whisper often mishears names |
| listen_window | 6.0 | Seconds to wait for a command after a bare wake phrase |
| device | auto | `auto`: use the GPU if it has enough free VRAM, otherwise CPU. `gpu`: always try the GPU (falls back to CPU if it fails to start). `cpu`: always CPU |
| gpu_device | 0 | Which GPU when there are several (0 = first discrete GPU) |
| vram_headroom_mb | 1024 | VRAM left for the game before `auto` will use the GPU |
| threads | 4 | CPU threads for speech recognition |
| min_level / level_ratio | 0.01 / 3.0 | Microphone gate: speech must be louder than max(min_level, background × ratio) |
| end_silence_ms / max_utterance_s | 700 / 10 | When a sentence ends |
| vad_threshold | 0.5 | Silero VAD: drops noise that isn't speech before Whisper runs |
| show_heard | true | Print every transcript, including ignored ones |

Whisper is primed with the wake phrases and the game's vehicle, brand and map names, so names like "LeGran" are recognised.

Each command shows an in-game message, "intent — result", e.g. "High beams — already on". The console shows the parsed intent and the raw resolver result.

### Commands understood (text mode)

| Area | Examples |
|---|---|
| Lights | high beams · low beams · headlights off · fog lights on · hazards off · indicate left · cancel indicator |
| Diffs | lock rear diff · unlock all diffs · lock front diff · lock inter axle diffs |
| Centre diff | centre diff 50% · centre diff auto (Sunburst) |
| Drivetrain | low range · high range · four wheel drive · 2wd |
| Assists | traction control off · tc on |
| Gears | park · drive · reverse · neutral · third gear · gear 2 |
| Readouts | temperatures · fuel · engine · speed · damage · what can I switch (also lists doors, bonnet, boot and extras such as JATO or an engine brake) |
| Engine | start the engine · start the car · fire it up · stop the engine · kill the engine (the starter is held for you until the engine catches) |
| Doors & panels | open the doors · open the driver door · close the rear left door · open the front doors · open the bonnet / hood · shut the boot / trunk / tailgate ("driver" = front left, "passenger" = front right) |
| Boost | jato on · fire the rockets · jato off · arm the nitrous · disarm nos (armed nitrous fires at full throttle) |
| Trucks | engine brake on · engine brake level 2 · jake brake off · retarder auto · retarder 60% · retarder off · parking brake on · release the handbrake · air brake on · raise / lower / stop the suspension |
| Vehicle | repair / fix / restore (repair, back to the road) · respawn / reset (back to the spawn point) |
| Spawn | spawn bruckell legran · spawn legran luxe v6 · replace car with a pigeon · add a percheron · list legran configs · list cars |
| Maps | list maps · what map is this · change map to west coast |
| Paint | make the white sunburst black · paint my car fire red · paint the pigeon matte black · change the colour of the bastion to blue |
| Time | set the time to 6 pm · half past 7 · make it sunset · noon · stop time · let time run · day length 10 minutes |
| Weather | clear skies · make it cloudy · overcast · light fog · no fog · wind 20 km/h · calm · light rain · heavy rain · no rain (maps with rain: Small Island, Industrial, Italy, Jungle Rock Island, East/West Coast USA, Driver Training, BeamNG.Earth — not the gridmaps) · stormy |
| Other environment | moon gravity · zero gravity · normal gravity · slow motion · half speed · real time |
| Help | help · what can I ask · what can you do · what commands are there · what vehicle / truck / satnav / map / level / game / weather / traffic commands are there · help with weather |
| Ask | what weather settings can I change · what time is it (lists every variable with its current value) |
| Traffic | spawn traffic · spawn ai · add 5 traffic cars · spawn 2 police cars · remove traffic |
| AI behaviour | make the white van chase me · police chase me · tell the blue pickup to follow me · stop the red taxi · make the cops run away · make all traffic drive aggressively · limit the taxi to 50 km/h |
| Places | list places · list gas stations · list charging stations · where can I go |
| Satnav | navigate to chinatown · set satnav to the drag strip · show me how to get to the quarry · where is the nearest gas station · find the nearest charging station · cancel satnav · clear the route |
| Distance | how far is the nearest service station · what's the distance to the dockyards · what direction is the race track (metres or km, plus N/NE/E…) |
| Autopilot | autopilot on (drives the satnav route; with no route it drives as traffic) · autopilot off · take me to the dockyards · drive me to the nearest gas station |
| Teleport | teleport to the highway · spawn at the race track (instant, and repairs the car) |
| AI spawn | spawn an ai van · spawn an ai legran police (extra AI car; you stay in yours) |
| Game | pause the game · unpause · close menu · back to the game (unpausing and closing menus is also done automatically at startup and after loading a map) |

"spawn <car>" with no config spawns the game's default config. If you drove that car earlier this session, the message names the config you last used. That memory lasts for the session only.

### Models

Models aren't used by text mode yet (voice arrives in milestone 2), but you can manage them now:

- At startup the exe prints the models folder and the **full path of every model file**, marking any not downloaded yet.
- Type `models` at the prompt to print that again.
- `beam-assist.exe --models` downloads anything missing, verifies checksums if set, and prints each full path and size. Downloads resume if interrupted.
- In `config.toml`:
  - `[models] dir = "models"` sets the folder. It is relative to the exe, or absolute (e.g. `"D:/AI/models"`).
  - Each `[models.<name>]` has `file` (the name inside `dir`), an optional `path` (the full path to a file stored anywhere; overrides dir + file), `url` (download source) and an optional `sha256`.

| Name | Default file | Size | Purpose |
|---|---|---|---|
| stt | ggml-base.en.bin | ~148 MB | Whisper speech-to-text |
| vad | ggml-silero-v5.1.2.bin | ~1 MB | Voice activity detection |
| embed + embed_config + embed_tokenizer | bge-small-en-v1.5 (safetensors) | ~133 MB | Intent router (stage B, run with Candle) |
| llm + llm_tokenizer | Qwen_Qwen3-0.6B-Q4_K_M.gguf | ~0.48 GB | Tie-breaks and slot filling only (picks from 2–3 candidates, never generates text) (stage C, Candle) |

### Layout

```
beam-assist.exe
config.toml          (generated)
models/              (milestone 2: downloaded automatically)
src/                 Rust source
lua/resolver.lua     vehicle-side resolver, compiled into the exe
lua/probe_*.lua      diagnostic probes
PROGRESS.md          design notes and findings
```
