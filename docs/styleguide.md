# Microchip Brutalism — GUI Style Guide
Inspired by a microchip die

---

## 1. Color Palette (Dark Grey / Black Industrial Theme)

### Primary Background
- **#0A0A0C** — near-black metallic  
  Use for: app background, large panels

### Secondary Backgrounds
- **#1A1C1E**  
- **#2A2C30**  
  Use for: nested blocks, compartments, data panels

### Highlight Metallics
- **#6C7078** (steel grey)  
- **#8C929B** (chip-trace grey)  
- **#B9C0C8** (aluminum highlight)

### Accents (use sparingly)
- **#7FFFD4** — electric mint, LED-like detail  
- **#A7D9FF** — icy blue highlight  
- **#C3A57A** — warm copper (optional wiring metaphor)

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
  - **#111216** or **#15171A**

### 6.2 Waveform View Style

**Background**
- Dark panel: **#101217 – #15171A**  
- Overlay subtle vertical grid lines (beats/frames):
  - Primary grid: **#242730** (1px)  
  - Secondary grid: **#1A1D24** (thinner or lower opacity)  
- Optional horizontal zero line: **#3C4048** (1px)

**Waveform Curve**
- Curved line is allowed here, but must feel “instrumental”:
  - 1–2px line
  - Primary color:
    - Default: **#A7D9FF** (icy blue)
    - Alternative highlight: **#7FFFD4** (electric mint) for selected/armed
  - No blur, no glow; if you need emphasis, use:
    - double-line effect (bright core, darker outline)
    - or stepped opacity segments

**Filling / Energy**
- Optional under-curve fill:
  - Very subtle, 5–15% opacity of the waveform color
  - Hard clipped at zero (no soft feathering)
- For selection regions:
  - Rectangular bands with sharp edges, color: **#262C36** or **#1E2830**

**Additional Details**
- Peaks or markers depicted as:
  - Thin vertical bars (no rounded markers)
  - Small blocky ticks along the top or bottom
- Zoom/pan handles: small square grips aligned to frame edges

### 6.3 Spectrogram / Frequency Displays

**Background**
- Same base as waveform (**#101217 – #15171A**)  
- Primary grid:
  - Vertical lines for time (**#232630**)  
  - Horizontal lines for frequency (**#242A33**)

**Color Mapping (Sci-Fi Hard Theme)**
- Use a **cold, high-tech palette** with minimal hues:
  - Low energy: **#111217 – #15171A**
  - Mid energy: **#3D4A5C**
  - High energy: **#A7D9FF**
  - Saturated peaks (very sparing): **#7FFFD4**
- Avoid rainbow spectrums; keep it within blue–cyan range for coherence.

**Rendering Style**
- Rectangular “pixels” or tiles:
  - Each time/frequency bin drawn as a small rectangular cell
  - Slightly hard, no blur on cell edges
- Optional horizontal banding noise to mimic sensor data

**Curves / Overlays**
- Overlays like EQ curves or analysis lines:
  - Thin 1px lines, **#C3A57A** (copper) or **#A7D9FF**
  - Allow smooth curves but:
    - No dot handles with circles — use small squares/diamonds
    - No glow; emphasize with line thickness or double-line effect

### 6.4 Other Data Views (Scopes, Vectors, Custom Displays)

**Oscilloscope / Lissajous**
- Frame same as waveform view  
- Curves allowed but:
  - Use crisp lines, no blur
  - Colors:
    - Main: **#A7D9FF**
    - Secondary/ghost: **#6C7078** with low opacity
- Optional trail effect:
  - Simulated with alpha decay, not blur

**Bar / Column Meters**
- Use vertical or horizontal **rectangular segments**  
- Segment colors:
  - Low: **#3D4A5C**
  - Mid: **#6C7078**
  - High: **#A7D9FF** / **#7FFFD4** for peaks
- Peak hold indicator: small rectangular cap, no rounded shapes

### 6.5 Display Chrome & Labeling

**Borders**
- Outer border: **#262A30** (1–2px)  
- Inner inset border: **#0A0A0C** or **#1A1C1E** to suggest depth

**Labels / Axis Text**
- Typeface: monospaced or technical-looking sans-serif  
- Color: **#B9C0C8** at 70–80% opacity  
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
