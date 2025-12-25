- lets design a smarter context changing ux system which understands the layout, so that alt+arrow key movement will correctly move around the 2d plane based on direction, without hardcoding all this.
the idea to to have contexts chromes, like the sample browser or waveform etc, to navigate these, the user can use alt+arrows.
navigation inside these contexts, like for example, navigating the browser list, the user can use plain arrow keys.

...

2. Persist kernel/pipeline cache if Burn/CubeCL supports it (might need an env flag). I can wire this
   in once we confirm the correct env var.

3. Keep embedding batch at 4 and serialize inference (now done) to avoid device‑lost crashes; larger
   batches are likely causing instability rather than speed.
