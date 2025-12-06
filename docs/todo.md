- lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve maintainability, add missing docs, find and resolve bugs, improve performance, etc.

--

\plan - add a clearly distinc visual rect in the lower 1 3rd of a selection area, turn this into a drag handle. users can use this drag handle to drag out the selected areas of the same, and they can then drop it into the sample list view, adding a new sample, cropped, to that list, saving it on disk. the user can also add it a collection, which should mark place the file into the current sample source folder on disk, as well as marking it as being added to the collection, listing it there as well in its view.

\plan - add some usage documentation to /docs/usage.md

\plan - add the hotkey F11 to switch between windowed and fullscreen mode
lets also make it possible for the user to drag down the topbar to unstick the fullscreen mode into a windowed mode
and add a 'go fullscreen' button to the topbar next to the x for closing

\plan - lets adjust the look and feel of everything so we have had rectangled everything, never round corners, if we must have soft corners, use a diagonal cut, very hard sci fi in terms of looks.
please review the styleguide in @styleguide.md and make a plan

\plan - lets adjust the way the trash/keep splitter ui works.
right now if we move up/down throught he list, we jump between all three columns.
lets instead isolate this to the focused column so we neatly go up and down that columns list only.
selecting an item in a column will focus that column.
if we move a sample from one column to another, dont keep that sample selected, instead select the next item in the current column.
 
\plan - when the user hovers the start or end of an audio selection, lets make a [ or ] icon visible at the bottom of the line, indicating the are now able to grab the edge and resize it.
