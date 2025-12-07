- lets do a housekeeping pass, clean up the codebase, reduce file lengths, improve maintainability, add missing docs, improve symbol naming, find and resolve bugs, improve performance, etc.

--
\plan - add some usage documentation to /docs/usage.md

\plan - add the hotkey F11 to switch between windowed and fullscreen mode
lets also make it possible for the user to drag down the topbar to unstick the fullscreen mode into a windowed mode
and add a 'go fullscreen' button to the topbar next to the x for closing

\plan - lets adjust the look and feel of everything so we have had rectangled everything, never round corners, if we must have soft corners, use a diagonal cut, very hard sci fi in terms of looks.
please review the styleguide in @styleguide.md and make a plan

\plan - when the user hovers the start or end of an audio selection, lets make a [ or ] icon visible at the bottom of the line, indicating the are now able to grab the edge and resize it.

- if the user disables looping while we are actively playing, stop looping after the current cycle. currently it just keep looping until we restart play.

- selecting items in the collection will somehow make the trash triage list autoscroll. can you remove this.

- currently if I drop a wave file item, it will start playing, lets stop that from happening

\plan - lets change our triage columns, instead of 3 columns, lets reduce it to a single collumn. lets add a red hue if a sasmple is trashed, or a green hue if its marked keep.
lets add a 'filter' option, which allows the user to only show trashed or kept items, or only regular untagged items, or all of them together.

- lets add a numbering column to the main sample list and to the collection items list showing the count of items

