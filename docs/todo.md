- currently moving up/down the samples list is slow, I think because it we are waiting on the sample to fully load first. lets decouple this so the user can move through samples quickly if they like. however, lets also implement strategies to make sample loading much fast, it seems quite slow now.

- lets add 2 more lists to the main list, so it turns into 3 colums, lets move trashed samples to the left column, removing them from the center list, and move keep samples to the right column, also removing them from the center list.
lets adjust left/right toggles so that center to left is 1 left tap, left to right is 2 right taps, right to center is 1 left tap, etc.
lets also change th tag visual to mark the entire sample list item of that sample with a soft color overlay of either green or red

- lets add a new sidebar on the very right, inside this add a system for collections
users can add new collection to this list. right below the collections list add a collection view which lists all samples inside of the currently selected collection if any.
then add a drag/drop feature which add the ability for users to pick up any sample, and drop it onto this list, which will add it to said collection.
this should be an additional flag, this action should dont move the sample, it should just add it to the collection. effectively tagging it as being part of said collection, or more collections.

