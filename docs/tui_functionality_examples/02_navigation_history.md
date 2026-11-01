# Example 02: parent navigation, history, and empty directories

## Given

The current directory is /home/frank/Documents at depth 1. It contains directories short_notes and scratch. short_notes contains an empty directory named proofs. scratch is also empty. Discovery is complete before each asserted listing.

## When and then

1. Type snt and press Enter. The picker enters short_notes with an empty first-visit query.
2. Type prf and press Enter. The picker enters proofs and shows an empty-directory message.
3. Type sh. Still empty-directory message.
3. Press Alt+h. The picker returns to short_notes and keeps query sh, resets scroll position, and shows an empty directory.
4. Press Alt+h. The picker moves to Documents, resets scroll position and highlight on short_notes (closest match), but also shows scratch with a lower score.
5. Press Escape to clear the query; highlight scratch using Alt+j and press Enter. The picker enters scratch despite it being empty.
6. Press Enter. The program outputs the path of scratch and halts.

## Assertions

- Enter in an empty directory returns all marked paths followed by the current directory if it is not already marked, then exits. With no marks, it returns only the current directory.
