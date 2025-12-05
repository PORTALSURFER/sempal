- lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve maintainability, add missing docs, find and resolve bugs, improve performance, etc.

--

- lets adjust the build script so it only bumps the version if we successfuly build, and not every run.

- lets add 2 more lists to the main list, so it turns into 3 colums, lets move trashed samples to the left column, removing them from the center list, and move keep samples to the right column, also removing them from the center list.
lets adjust left/right toggles so that center to left is 1 left tap, left to right is 2 right taps, right to center is 1 left tap, etc.
lets also change th tag visual to mark the entire sample list item of that sample with a soft color overlay of either green or red

- lets add a new sidebar on the very right, inside this add a system for collections
users can add new collection to this list. right below the collections list add a collection view which lists all samples inside of the currently selected collection if any.
then add a drag/drop feature which add the ability for users to pick up any sample, and drop it onto this list, which will add it to said collection.
this should be an additional flag, this action should dont move the sample, it should just add it to the collection. effectively tagging it as being part of said collection, or more collections.

- add a clearly distinc visual rect in the lower 1 3rd of a selection area, turn this into a drag handle. users can use this drag handle to drag out the selected areas of the same, and they can then drop it into the sample list view, adding a new sample, cropped, to that list, saving it on disk. the user can also add it a collection, which should mark place the file into the current sample source folder on disk, as well as marking it as being added to the collection, listing it there as well in its view.

- add some usage documentation to /docs/usage.md

- add the hotkey F11 to switch between windowed and fullscreen mode
lets also make it possible for the user to drag down the topbar to unstick the fullscreen mode into a windowed mode
and add a 'go fullscreen' button to the topbar next to the x for closing

- lets adjust the look and feel of everything so we have had rectangled everything, never round corners, if we must have soft corners, use a diagonal cut, very hard sci fi in terms of looks. 

