- lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve maintainability, add missing docs, improve symbol naming, find and resolve bugs, improve performance, etc.

--
lets change our app logo from the default egui to ./assets/logo3.png

increase the quality of our waveform rendering, it's very pixelated right now.

\plan - add some usage documentation to /docs/usage.md

\plan - add the hotkey F11 to switch between windowed and fullscreen mode
lets make fullscreen mode the default mode

\plan - lets adjust the look and feel of everything so we have had rectangled everything, never round corners, if we must have soft corners, use a diagonal cut, very hard sci fi in terms of looks.
please review the styleguide in @styleguide.md and make a plan

\plan - when the user hovers the start or end of an audio selection, lets make a [ or ] icon visible at the bottom of the line, indicating the are now able to grab the edge and resize it.

- if the user disables looping while we are actively playing, stop looping after the current cycle. currently it just keep looping until we restart play.

- selecting items in the collection will somehow make the trash triage list autoscroll. can you remove this.

- currently if I drop a wave file item, it will start playing, lets stop that from happening

- lets add a numbering column to the main sample list and to the collection items list showing the count of items

- lets add a toolbar with an options menu, lets add an option to choose a 'trash' folder on disk.
lets add a feature users can use to trash all trashed files (tagged), add a warning asking the user if they are sure, this should physically move all the trash marked files to this trash folder.
lets also add a 'take out trash' option, also with a warning, which should hard delete all files in the trash folder.
lets also add an option to open the trash folder in the OS file explorer.
