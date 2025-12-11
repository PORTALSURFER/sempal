- lets do a housekeeping pass, clean up the codebase, cleanup warnings, reduce file lengths, improve DRYness, improve maintainability, collapse large structs/objects into clearly named smaller objects, add missing docs, improve symbol naming, find and resolve bugs, improve performance, etc.
lets then write every task you find into @todo.md as a new todo item

--

- in the folder browser add a . entry at the very top, sticky, to represent the root of the source target, so that user can create a new folder at the source target level from this for example

- lets change the new folder creation ui, the uses should select a target for where to create the folder, once activated, add a dummy folder item in place, and make it editable text so users can name the folder in place, using enter to commit, or esc/click outside to cancel.

- if the user tried to drop a sample item into a collection when no collection is active, give the user clear feedback that this is not possible and they need to create a collection first.

-lets move the item count label in the sample browser to the far right side right budding up against the right sidebar there

- when the playhead reached the end, hide it

- for some samples, the playhead will stop before the end of the sample, at least visually, please audit this.

- add a little dice logo in the toolbar at the top of the sample browser (where the search bar is etc). users can click it to play a random sample, and can shift+click it to toggle sticky random mode.
