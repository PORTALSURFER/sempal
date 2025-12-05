- lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve maintainability, add missing docs, find and resolve bugs, improve performance, etc.

--

\plan - lets add a new sidebar on the very right, inside this add a system for collections
users can add new collection to this list. right below the collections list add a collection view which lists all samples inside of the currently selected collection if any.
then add a drag/drop feature which add the ability for users to pick up any sample, and drop it onto this list, which will add it to said collection.
this should be an additional flag, this action should dont move the sample, it should just add it to the collection. effectively tagging it as being part of said collection, or more collections.

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

\plan - the ui is very slow when selecting samples in the sample source lists, lets review which parts currently are slowing things down, find the bottlenecks, and come up with solutions for drastically improving performance, and list others which might need a redesign in terms of features.