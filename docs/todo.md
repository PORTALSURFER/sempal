- zoom sensitivity settings are a bit weird, its now super high, almost full, ander higher will make zooming slower
lowering the value and zoom starts going really fast. lets make this more intuitive
low values should be slow zoom, high values fast zoom.

- renaming items in the sample browser does not work yet. it will also draw the input text on top of the existing item right now, making it very hard to read what the user is writing
please align it much more with how folder renaming works. an for dryness, lets merge/reuse what we can.

- if I replay samples quickly so we restart while its still playing, I hear clicks, lets fix that so we fade out very quickly right before we restart

- when I undo flagging of a sample in the browser, it should refocus the sample again also

- when we move trashed files to the trash folder, we properly get a progress bar, but its drawn underneath the statusbar breaking the layout. lets instead place the progress bar insie of the statusbar, to the left of the volume settings

##
lets audit the codebase, focus on improving maintainability, improve dryness, more
effecient architectures, etc


