# Microchip Brutalism — GUI Style Guide
Inspired by a microchip die

---

## 1. Color Palette (Current App Palette)

### Core surfaces
- **#0C0B0A** — primary background for the app canvas and modal bases  
- **#141210** — secondary background for panels and cards  
- **#1C1A17** — tertiary background for controls and list rows to add depth  
- **#2C2824** — panel outline for outer frames and strong dividers  
- **#37322D** — grid strong for primary lines in displays and separators  
- **#2A2622** — grid soft for secondary lines and subtle row backing

### Text
- **#E0E3EA** — primary text for labels, buttons, and inputs  
- **#A6ADB8** — muted text for helper copy and secondary metadata  
- **#FFFFFF** — high-contrast text on tinted badges/overlays

### Accents and feedback
- **#98AC9E** — accent mint; selection fills, cursor trails, positive focus  
- **#A8967E** — accent ice; focus strokes, hyperlinks, keyboard focus rings  
- **#BA946C** — accent copper; waveform highlight/selection tone and warm accent  
- **#BACCBA** — success state fills and badges  
- **#C29E6C** — warning foregrounds and strokes  
- **#B87070** — destructive/error strokes and text  
- **#CCB084** — soft warning/informational fills

### Badges and chips
- Idle **#2A2E36**; Busy **#A49274**; Info **#9CB09E**; Warning **#C09E70**; Error **#B87070**

### Interaction overlays & triage
- Drag highlight **#B49C7E** (outline alpha varies by state)  
- Duplicate hover: fill **#30343A**, stroke **#A49274**  
- Triage: Trash **#9E6660** (subtle **#744E4A**), Keep **#7E9C7E**  
- Missing marker **#CC8484**  
- Palette source of truth: `src/egui_app/ui/style.rs` — keep these values in sync when the theme shifts.

---

## 2. Geometry / Shape Language

### Core Shape Rules
- Only **rectilinear** shapes.
- **No curves**, no rounded corners, for UI chrome.
- To “soften” corners, use a **45° diagonal bevel** instead.
- Dense layers of rectangular blocks.
- Strong separation lines and strict grid behavior.

### Structural Forms Derived From the Chip
- Long vertical stacks  
- Horizontal banding  
- Checkerboard micro-patterns  
- Modular “bays” and compartments  
- Highly partitioned containers

### Line Geometry
- 1–2px micro-lines  
- 4–8px structural divider lines  
- 45° diagonals for transitions or “bridges”

---

## 3. Layout Principles

### A. Dense Structure
- Minimal empty space  
- Tight spacing  
- Everything looks *purpose-built* and mechanical

### B. Module-within-Module Design
- Outer frame  
- Subdivided sections  
- Micro-complex nesting

### C. Repetition
- Repeated bars  
- Repeated squares  
- Repeated micro-lines for rhythm and structure

### D. Asymmetry With Balance
- Not perfectly symmetrical  
- Variations in density  
- Intentional irregularities

---

## 4. Surface Texture Simulation

### Microline Textures
- Fine horizontal/vertical streaks  
- Use for: buttons, sliders, small panels

### Scanline / Interference Patterns
- Good for headers, status bars, separators

### Gridfill Textures
- 2x2 or 4x4 subtle pixel grids  
- Used to differentiate UI groups

---

## 5. Components Design Language

### Buttons
- Rectangles only  
- No curved corners  
- Optional diagonal bevel (45°)  
- Thin 1px outline  
- Pressed state: inset 1px inner shadow

### Sliders
- Track: long, narrow, rectilinear, microtextured  
- Handle: square block with possible 45° cut  
- Tick marks: 1px micro-grids

### Panels
- Thick outer frames  
- Subsection grids  
- Repeated vertical dividers

### Tabs / Navigation
- Tall, thin rectangular tabs  
- Resemble microchip partitions  
- Depth indicated by offset shading

### Meters / Waveforms (UI chrome)
- Sharp, blocky containers  
- No curved shapes in the surrounding chrome  
- Monochrome grey stack visuals

---

## 6. Displays / Views (Data Visualizations)

> **Rule:** Curves are only allowed *inside* dedicated data displays (waveforms, spectrograms, analyzers). All surrounding chrome must still follow the hard, rectilinear style.

### 6.1 Display Frames

- Display areas (e.g. waveform view, spectrogram, meters) sit inside:
  - A **rigid rectangular frame**
  - With 1–2 nested inner borders to mimic multi-layer chip regions
  - Optional 45° bevels on outer corners only if you need visual hierarchy
- Use a slightly lighter background than the main app:
  - **#141210 – #1C1A17**

### 6.2 Waveform View Style

**Background**
- Dark panel: **#0C0B0A – #1C1A17**  
- Overlay subtle vertical grid lines (beats/frames):
  - Primary grid: **#37322D** (1px)  
  - Secondary grid: **#2A2622** (thinner or lower opacity)  
- Optional horizontal zero line: **#2C2824** (1px)

**Waveform Curve**
- Curved line is allowed here, but must feel “instrumental”:
  - 1–2px line
  - Primary color:
    - Default: **#BA946C** (accent copper)
    - Alternative highlight: **#98AC9E** (accent mint) for selected/armed
  - No blur, no glow; if you need emphasis, use:
    - double-line effect (bright core, darker outline)
    - or stepped opacity segments

**Filling / Energy**
- Optional under-curve fill:
  - Very subtle, 5–15% opacity of the waveform color (copper by default)
  - Hard clipped at zero (no soft feathering)
- For selection regions:
  - Rectangular bands with sharp edges, using **#BA946C** at low opacity (handles rise to 80–90% opacity)

**Additional Details**
- Peaks or markers depicted as:
  - Thin vertical bars (no rounded markers), **#A8967E**
  - Small blocky ticks along the top or bottom
- Zoom/pan handles: small square grips aligned to frame edges

### 6.3 Spectrogram / Frequency Displays

**Background**
- Same base as waveform (**#0C0B0A – #1C1A17**)  
- Primary grid:
  - Vertical lines for time (**#2A2622**)  
  - Horizontal lines for frequency (**#37322D**)

**Color Mapping (Sci-Fi Hard Theme)**
- Use a **cold, high-tech palette** with minimal hues:
  - Low energy: **#0C0B0A – #1C1A17**
  - Mid energy: **#30343A**
  - High energy: **#98AC9E**
  - Saturated peaks (very sparing): **#BA946C**
- Avoid rainbow spectrums; keep it within blue–cyan range for coherence.

**Rendering Style**
- Rectangular “pixels” or tiles:
  - Each time/frequency bin drawn as a small rectangular cell
  - Slightly hard, no blur on cell edges
- Optional horizontal banding noise to mimic sensor data

**Curves / Overlays**
- Overlays like EQ curves or analysis lines:
  - Thin 1px lines, **#BA946C** (copper) or **#98AC9E** (mint)
  - Allow smooth curves but:
    - No dot handles with circles — use small squares/diamonds
    - No glow; emphasize with line thickness or double-line effect

### 6.4 Other Data Views (Scopes, Vectors, Custom Displays)

**Oscilloscope / Lissajous**
- Frame same as waveform view  
- Curves allowed but:
  - Use crisp lines, no blur
  - Colors:
    - Main: **#BA946C** (accent copper)
    - Secondary/ghost: **#98AC9E** with low opacity
- Optional trail effect:
  - Simulated with alpha decay, not blur

**Bar / Column Meters**
- Use vertical or horizontal **rectangular segments**  
- Segment colors:
  - Low: **#30343A**
  - Mid: **#37322D**
  - High: **#98AC9E** / **#BA946C** for peaks
- Peak hold indicator: small rectangular cap, no rounded shapes

### 6.5 Display Chrome & Labeling

**Borders**
- Outer border: **#2C2824** (1–2px)  
- Inner inset border: **#0C0B0A** or **#141210** to suggest depth

**Labels / Axis Text**
- Typeface: monospaced or technical-looking sans-serif  
- Color: **#A6ADB8** at 70–80% opacity  
- Alignment:
  - Frequency labels: left or right edge  
  - Time labels: bottom edge  
- Use blocky separators (short lines) instead of dots or circles

---

## 7. Lighting & Shading

### General Aesthetic
- Mostly flat shading  
- Subtle metallic reflections  
- Sharp edge highlights (1–2px)  
- Depth conveyed by layered geometry, not blur

### Avoid:
- Blur  
- Glow (except tiny LED accents)  
- Soft gradients  

Only micro-linear gradients allowed.

---

## 8. Interaction Feel

### Behavioral Personality
Interactions should feel:
- Mechanical  
- Precise  
- Hard-edged  
- Instant and snappy

### Allowed Interactions
- Sliding blocks  
- Hard toggles  
- Snap-open compartments

### Prohibited
- Bounce animations  
- Soft fades  
- Curved motion paths

---

## 9. High-Level Style Keywords

- **Microchip Brutalism**  
- Rectilinear Density  
- Industrial Metal  
- Machine-Logic Aesthetic  
- 45° Geometry  
- Partitioned Complexity  
- High-Frequency Patterns  
- Dark Circuit Board  
- Cold, Technical, Mechanical  

---
