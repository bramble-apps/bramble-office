# Questions for the Mac session (Numbers 15.4)

Asked 2026-11-10 from the table clipboard / typing round. None of these is in out-N1 … out-N30.
Values implements the answer given in brackets for now; please check each in Numbers and say
where it differs (a screenshot or AppleScript readout of the cells is enough).

## Copy and paste of cells

1. **Borders.** Copy a cell with a custom border (Format > Cell > Border, e.g. 2 pt red bottom)
   and paste it over a plain cell. Does the border come along? Copy a plain cell and paste it over
   a cell with a custom border: is the border removed? Are the copied block's outer edges pasted
   too, or only the lines inside it? [Values: every edge of each copied cell comes along and
   replaces the destination's; lines that are the source table's usual grid become the
   destination table's grid.]
2. **Header cell into the body and back.** N18 P10 says a header cell pasted into the body is body
   text. Is that true for its fill and bold too, when the header cell has no fill/bold of its own?
   And a body cell pasted into a header row: does it become header-styled, or keep the body look?
   [Values: only what differs from the cell's own region (header row / column, footer, body)
   travels; the rest takes the destination region's look.]
3. **Different table styles.** Copy cells from a table in style 1 to a table in style 3
   (Table inspector > Table Styles). Do the copied cells keep the first table's fills, text
   colour and grid lines, or take the second table's? [Values: as in 2, the destination's
   region look unless the cell had its own.]
4. **Empty formatted cells.** Make an empty cell bold, red text, centred (no value), copy it,
   paste over another empty cell, then type in the pasted cell. Is the typing bold, red, centred?
   [Values: fill, borders, wrap, alignment come along; bold / colour of an EMPTY cell with no fill
   can't be stored by Keydeck's model yet, so they don't.]
5. **Paste over a selection bigger than the copy that cuts a copied merge.** Copy A1:B2 where
   A1:B1 is merged, select a 3x3 range and paste: is the merge made in the partial tile?
   [Values: only merges that fit whole inside the selection are made again.]

## Merging cells with content

6. Select B2:C3 where only C3 holds a value (a number in Currency format, with a fill) and
   Table > Merge Cells. Does the merged cell show C3's value as a number with its format and fill,
   or text? [Values: the lone value moves to the merged cell unchanged: value, format, formula,
   fill and text style.]
7. Merge cells that hold two numbers (B2 = 1, C2 = 2): "1<TAB>2" text, as N18 P11 found for text?
   And a column merge where the top cell is empty but the lower one has a fill: whose fill does
   the merged cell take (Apple's help says the top cell's for a column, the leftmost for a row)?
   [Values: joined as text; the first cell's fill.]

## Rows growing / the add-rows handle

8. **A row whose height was set by hand** (drag the row line to 30 pt), then type three lines in a
   wrapping cell of that row. Does the row grow, or does the text get cut off (with Numbers'
   clipping indicator, the small "+" at the bottom of the cell)? And dragging a row shorter than
   its text: allowed, or does it stop at the text's height? [Values now: every row grows to fit
   its lines, its own height being the least it takes. If Numbers clips instead, we need the
   clipping indicator's look.]
9. **Wrap off, typing a long line.** In a cell with Wrap Text off, type past the cell's width:
   does the editor widen over the next cells (empty or not), and after Return does the text run
   over the empty cells to the right and get cut at the first filled one? [Values: the editor
   widens; the sheet draws as Keydeck's renderer does.]
10. **The "=" handle drag.** N21 13 says rows are added step by step while dragging. Is anything
    shown next to the pointer (a count like "+3")? Dragging back up past rows that hold values:
    does it stop at the last filled row, or delete them? [Values: no label; rows are added and
    removed live as the pointer passes each 20 pt; only empty rows at the end are taken away.]

## Pasting a picture

11. With a **table cell selected** (not editing), paste a PNG copied from Preview: does it become
    the cell's Image Fill? Which technique does the Fill inspector show (Scale to Fill / Scale to
    Fit / Original Size)? Same with the cell being **edited** (caret in it). [Values: Image Fill,
    Scale to Fill, in both cases.]
12. With **nothing selected**: where does the picture land and at what size (a 3000 px screenshot:
    full size, or scaled to fit the visible area)? With a **shape or text box selected**?
    [Values: placed in the middle of the view at its pixel size, selected; with an object
    selected, the same.]
13. Copy a cell range in **Excel / LibreOffice** (they put a picture of the cells on the clipboard
    along with the text): does Numbers paste cells or a picture? [Values: cells (text with tabs or
    line breaks wins over a picture).]
