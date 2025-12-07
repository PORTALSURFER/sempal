\plan - lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve DRYness, improve maintainability, collapse large structs/objects into clearly named smaller objects, add missing docs, improve symbol naming, find and resolve bugs, improve performance, etc.

--


\plan - lets adjust the look and feel of everything so we have had rectangles for everything, never round corners, if we must have soft corners, use a diagonal cut, very hard sci fi in terms of looks.
please review the styleguide in @styleguide.md and make a plan following it.

- if the user disables looping while we are actively playing, stop looping after the current cycle. currently it just keep looping until we restart play.

- lets add a numbering column to the main sample list and to the collection items list showing the count of items

\plan - lets add a toolbar with an options menu, lets add an option to choose a 'trash' folder on disk.
lets add a feature users can use to trash all trashed files (tagged), add a warning asking the user if they are sure, this should physically move all the trash marked files to this trash folder.
lets also add a 'take out trash' option, also with a warning, which should hard delete all files in the trash folder.
lets also add an option to open the trash folder in the OS file explorer.

- lets add the trash/keep tagging system to the collection list as well. with full coloring etc.

\plan - lets add an alt+drag feature for the system which allows dragging waveform selections to extract that part of the audio. but in this case, cut the cropped part out of the original sample. Dont destroy the original sample however, save this edited 'original' as a new version. selecting that new one instead.

\plan - add the ability to select multiple sample items, ctrl+mouseclick should add another item, shift+mouseclick should extend the current select to this item.
shift+up/down should grow the selection
pressing x should mark the item selected in a way that allows the user to focus another item, and mark it selected with x as well, similart to ctrl+click.
lets design a difference between a selected and a focused item with this change.
behavior of regular navigation shuold stay the same, so a 'focused' item should autoplay etc.

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
