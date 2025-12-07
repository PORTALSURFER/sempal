- lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve maintainability, add missing docs, improve symbol naming, find and resolve bugs, improve performance, etc.

--

increase the quality of our waveform rendering, it's very pixelated right now.

\plan - add some usage documentation to /docs/usage.md

\plan - add the hotkey F11 to switch between windowed and fullscreen mode
lets make fullscreen mode the default mode

\plan - lets adjust the look and feel of everything so we have had rectangled everything, never round corners, if we must have soft corners, use a diagonal cut, very hard sci fi in terms of looks.
please review the styleguide in @styleguide.md and make a plan

\plan - when the user hovers the start or end of an audio selection, lets make a [ or ] icon visible at the bottom of the line, indicating the are now able to grab the edge and resize it.

- if the user disables looping while we are actively playing, stop looping after the current cycle. currently it just keep looping until we restart play.

- lets add a numbering column to the main sample list and to the collection items list showing the count of items

- lets add a toolbar with an options menu, lets add an option to choose a 'trash' folder on disk.
lets add a feature users can use to trash all trashed files (tagged), add a warning asking the user if they are sure, this should physically move all the trash marked files to this trash folder.
lets also add a 'take out trash' option, also with a warning, which should hard delete all files in the trash folder.
lets also add an option to open the trash folder in the OS file explorer.

- if im drawing a new selection of our waveform, but move quickly outside of the waveform frame, we stop sizing it, while not fully touching the wall on that side. lets make it so we keep drawing/adjusting the selection until we release the mouse, not just stop when we move out of the frame

- lets add the trash/keep tagging system to the collection list as well. 

- lets add an alt+drag feature for the system which allows dragging waveform selections to extract that part of the audio. but in this case, cut the cropped part out of the original sample. Dont destroy the original sample however, save this edited 'original' as a new version. selecting that new one instead.