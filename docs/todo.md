\plan - lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve DRYness, improve maintainability, collapse large structs/objects into clearly named smaller objects, add missing docs, improve symbol naming, find and resolve bugs, improve performance, etc.

--

\plan - when I resize the sides of the waveform selection, there is a slight stickyness at first, I need to move the mouse a couple pixels before it unlocks and we actually start resizing, this is very annoying for precise tweaks, lets improve this so its butter smooth and instant.

\plan - add a strong and solid logging crate, using tracing, to log to console and file.

\plan - let add a context menu to our audo selection block, with an option to crop, also add an option to trim to users can delete parts of the audio like silence. lets also add an option to add a simple fadeout to null, either left to right, or right to left, lets use / or \ icons to make the direction clear to users. the null.
lets also add an opton to 'mute' which will null the entire selection, fading back into the audio at the far edges with a default of a 5 ms fade.

\plan - add similarity search systems

\plan - while in loop mode, if we play with a mouseclick, we should still end up looping, right now this method will just play oneshot style. only playing with spacebar will loop.

\plan - lets clean up the config location names so we don't get this double sempal/sempal structure. lets make it simple .sempal/
So in windows it would end up as %APPDATA%\Roaming\.sempal\config.json

\plan - I noticed we currently use our config file to store collection members? lets move this to use our sqlite db instead.
the config file should be a lean, app only file, not to store data in. just to set app flags, etc, maybe color themes in the future, etc.
lets also turn it from json into toml.
and lets add migration code to find and convert the current config.json format/path to our new system

\plan - add ability to select the audio output device, sample rate, and other typical audio output settings in a nice options menu.

- if we are playing, lets make hitting esc stop playback

- lets add support for hotkey chords, then lets add 'gw' to goto waveform, to set user focus to the waveform.
in context of waveform focus, left/right arrows should move the playhead, lets add a stepwise motion, which is always the same visual size. make up/down zoom in/out, keeping the playhead at the center of zoom.
shift+left/right to create a selection.  [ and ] to push the selection sides outward on either side. shift+[/] to push either side of the selection inward.
lets add 'gs' to focus the source samples list, 'gc' to focus collection samples list, 'gS' to focus the source list, 'gC' to focus the collections list.

