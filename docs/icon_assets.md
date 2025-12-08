# Icon Asset Generation

The Windows taskbar icon and executable resource are derived from the same PNG artwork as the in-app icon:

- Source image: `assets/logo3.png`
- Generated file: `assets/logo3.ico`

To regenerate the ICO (after updating the PNG), run ImageMagick from the project root:

```powershell
magick assets/logo3.png -define icon:auto-resize=256,128,96,64,48,32,24,16 assets/logo3.ico
```

The command emits the multi-resolution `.ico` file that Windows expects for taskbar and Start Menu entries.
