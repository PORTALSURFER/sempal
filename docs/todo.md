\plan - lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve DRYness, improve maintainability, collapse large structs/objects into clearly named smaller objects, add missing docs, improve symbol naming, find and resolve bugs, improve performance, etc.

--

\plan - currently the triageflags will mark the entire item with a colored overlay, lets instead of coloring the entire item, only mark a small rect on the far right, to keep things visually cleaner.

\plan - lets clean up the config location names so we don't get this double sempal/sempal structure. lets make it simple .sempal/
So in windows it would end up as %APPDATA%\Roaming\.sempal\config.json

\plan - I noticed we currently use our config file to store collection members? lets move this to use our sqlite db instead.
the config file should be a lean, app only file, not to store data in. just to set app flags, etc, maybe color themes in the future, etc.
lets also turn it from json into toml.
and lets add migration code to find and convert the current config.json format/path to our new system

\plan - dont show the extensions or folders in our item lists, both for collections and for the regular sample list, show only the sample name

\plan - our icon is sometimes not visible in the windows taskbar, it shows some default windows app icon. but sometimes it does work oddly.

\plan - add ability to select the audio output device, sample rate, and other typical audio output settings in a nice options menu.

\plan - for missing samples, lets add a missing icon clearly informing the user visually, keep then in the database though. later wil will add features to try and recover missing items.
right now also if we select a missing audio file, the waveform still renders the previously selected file, and playback still plays that buffer, lets clear the waveform and add a clear message that we have a missing file selected here too, and play nothing.

\plan - lets add a contextual hotkey system. based on user focus, so we also need to add a user focus system if we dont have one yet.
as a first hotkey, lets add 'x' to select the focused sample, 'n' to normalize the sample, 'd' to delete the sample, 'c' to add it to the current collection.  lets also add a hotkey ctrl+/ to show a visual popup, listing the currently available hotkeys in context of the currently focused item, global hotkeys if any.

\plan - when I resize the sides of the waveform selection, there is a slight stickyness at first, I need to move the mouse a couple pixels before it unlocks and we actually start resizing, this is very annoying for precise tweaks, lets improve this so its butter smooth and instant.

\plan - lets prefer using vulkan if egui and the users OS support it, which is windows currently.

\plan - add a litte icon or graphic in the top left corner of our ui when in fullscreen mode, to users can toggle back to windowed mode using it instead of f11 as an alternative option

- hover highlighting of items is hard to see, lets make it more visually intense, also, when items are marked with triageflags, the hover highlight seems to be not visible at all, make sure here too we get clearly visible hover highlighting.

\plan - add a strong and solid logging crate, using tracing, to log to console and file.

\plan - let add a context menu to our audo selection block, with an option to crop, also add an option to trim to users can delete parts of the audio like silence. lets also add an option to add a simple fadeout to null, either left to right, or right to left, lets use / or \ icons to make the direction clear to users. the null.
lets also add an opton to 'mute' which will null the entire selection, fading back into the audio at the far edges with a default of a 5 ms fade.

\plan - lets add the ability to drag drop folder externally, from the OS explorer for example, onto the sample source list, to add a new sample source 

\plan - add similarity search systems

\plan - while in loop mode, if we play with a mouseclick, we should still end up looping, right now this method will just play oneshot style. only playing with spacebar will loop.