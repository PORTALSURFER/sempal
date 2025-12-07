- lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve maintainability, add missing docs, improve symbol naming, find and resolve bugs, improve performance, etc.

--
\plan - when the user hovers the start or end of an audio selection, lets make a [ or ] icon visible at the bottom of the line, indicating the are now able to grab the edge and resize it.

\plan - add the hotkey F11 to switch between windowed and fullscreen mode
lets make fullscreen mode the default mode

\plan - lets adjust the look and feel of everything so we have had rectangles for everything, never round corners, if we must have soft corners, use a diagonal cut, very hard sci fi in terms of looks.
please review the styleguide in @styleguide.md and make a plan following it.

- if the user disables looping while we are actively playing, stop looping after the current cycle. currently it just keep looping until we restart play.

- lets add a numbering column to the main sample list and to the collection items list showing the count of items

- lets add a toolbar with an options menu, lets add an option to choose a 'trash' folder on disk.
lets add a feature users can use to trash all trashed files (tagged), add a warning asking the user if they are sure, this should physically move all the trash marked files to this trash folder.
lets also add a 'take out trash' option, also with a warning, which should hard delete all files in the trash folder.
lets also add an option to open the trash folder in the OS file explorer.

- if im drawing a new selection of our waveform, but move quickly outside of the waveform frame, we stop sizing it, while not fully touching the wall on that side. lets make it so we keep drawing/adjusting the selection until we release the mouse, not just stop when we move out of the frame

- lets add the trash/keep tagging system to the collection list as well. with full coloring etc.

- lets add an alt+drag feature for the system which allows dragging waveform selections to extract that part of the audio. but in this case, cut the cropped part out of the original sample. Dont destroy the original sample however, save this edited 'original' as a new version. selecting that new one instead.

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

\plan - make the collection sameple list context menu with all features also work for the regular sample list

\plan - dont show the extensions or folders in our item lists, both for collections and for the regular sample list, show only the sample name

\plan - our icon is not visible in the windows taskbar, it shows some default windows app icon right now.