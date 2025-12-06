- lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve maintainability, add missing docs, find and resolve bugs, improve performance, etc.

--

\plan - add a context menu to the collection items, in it, add the option for the user to select an export path on disk. collections should export their files to this folder automatically whenever a file is added to it. also delete again if a file is removed.
also add an option to 'refresh export' so that if we add or removed files externally, the collection is updated accordingly.

\plan - add a clearly distinc visual rect in the lower 1 3rd of a selection area, turn this into a drag handle. users can use this drag handle to drag out the selected areas of the same, and they can then drop it into the sample list view, adding a new sample, cropped, to that list, saving it on disk. the user can also add it a collection, which should mark place the file into the current sample source folder on disk, as well as marking it as being added to the collection, listing it there as well in its view.

\plan - add some usage documentation to /docs/usage.md

\plan - add the hotkey F11 to switch between windowed and fullscreen mode
lets also make it possible for the user to drag down the topbar to unstick the fullscreen mode into a windowed mode
and add a 'go fullscreen' button to the topbar next to the x for closing

\plan - lets adjust the look and feel of everything so we have had rectangled everything, never round corners, if we must have soft corners, use a diagonal cut, very hard sci fi in terms of looks.
please review the styleguide in @styleguide.md and make a plan

\plan - when the user hovers the start or end of an audio selection, lets make a [ or ] icon visible at the bottom of the line, indicating the are now able to grab the edge and resize it.

- if the user disables looping while we are actively playing, stop looping after the current cycle. currently it just keep looping until we restart play.
