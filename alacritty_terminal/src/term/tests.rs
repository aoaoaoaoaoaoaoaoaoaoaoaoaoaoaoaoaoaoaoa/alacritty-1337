use super::*;

use std::mem;

use crate::event::VoidListener;
use crate::grid::{Grid, Scroll};
use crate::index::{Column, Point, Side};
use crate::selection::{Selection, SelectionType};
use crate::term::cell::{Cell, Flags};
use crate::term::test::TermSize;
use crate::vte::ansi::{self, CharsetIndex, Handler, StandardCharset};

#[test]
fn scroll_display_page_up() {
    let size = TermSize::new(5, 10);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Create 11 lines of scrollback.
    for _ in 0..20 {
        term.newline();
    }

    // Scrollable amount to top is 11.
    term.scroll_display(Scroll::PageUp);
    assert_eq!(term.vi_mode_cursor.point, Point::new(Line(-1), Column(0)));
    assert_eq!(term.grid.display_offset(), 10);

    // Scrollable amount to top is 1.
    term.scroll_display(Scroll::PageUp);
    assert_eq!(term.vi_mode_cursor.point, Point::new(Line(-2), Column(0)));
    assert_eq!(term.grid.display_offset(), 11);

    // Scrollable amount to top is 0.
    term.scroll_display(Scroll::PageUp);
    assert_eq!(term.vi_mode_cursor.point, Point::new(Line(-2), Column(0)));
    assert_eq!(term.grid.display_offset(), 11);
}

#[test]
fn scroll_display_page_down() {
    let size = TermSize::new(5, 10);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Create 11 lines of scrollback.
    for _ in 0..20 {
        term.newline();
    }

    // Change display_offset to topmost.
    term.grid_mut().scroll_display(Scroll::Top);
    term.vi_mode_cursor = ViModeCursor::new(Point::new(Line(-11), Column(0)));

    // Scrollable amount to bottom is 11.
    term.scroll_display(Scroll::PageDown);
    assert_eq!(term.vi_mode_cursor.point, Point::new(Line(-1), Column(0)));
    assert_eq!(term.grid.display_offset(), 1);

    // Scrollable amount to bottom is 1.
    term.scroll_display(Scroll::PageDown);
    assert_eq!(term.vi_mode_cursor.point, Point::new(Line(0), Column(0)));
    assert_eq!(term.grid.display_offset(), 0);

    // Scrollable amount to bottom is 0.
    term.scroll_display(Scroll::PageDown);
    assert_eq!(term.vi_mode_cursor.point, Point::new(Line(0), Column(0)));
    assert_eq!(term.grid.display_offset(), 0);
}

#[test]
fn simple_selection_works() {
    let size = TermSize::new(5, 5);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let grid = term.grid_mut();
    for i in 0..4 {
        if i == 1 {
            continue;
        }

        grid[Line(i)][Column(0)].c = '"';

        for j in 1..4 {
            grid[Line(i)][Column(j)].c = 'a';
        }

        grid[Line(i)][Column(4)].c = '"';
    }
    grid[Line(2)][Column(0)].c = ' ';
    grid[Line(2)][Column(4)].c = ' ';
    grid[Line(2)][Column(4)].flags.insert(Flags::WRAPLINE);
    grid[Line(3)][Column(0)].c = ' ';

    // Multiple lines contain an empty line.
    term.selection = Some(Selection::new(
        SelectionType::Simple,
        Point { line: Line(0), column: Column(0) },
        Side::Left,
    ));
    if let Some(s) = term.selection.as_mut() {
        s.update(Point { line: Line(2), column: Column(4) }, Side::Right);
    }
    assert_eq!(term.selection_to_string(), Some(String::from("\"aaa\"\n\n aaa ")));

    // A wrapline.
    term.selection = Some(Selection::new(
        SelectionType::Simple,
        Point { line: Line(2), column: Column(0) },
        Side::Left,
    ));
    if let Some(s) = term.selection.as_mut() {
        s.update(Point { line: Line(3), column: Column(4) }, Side::Right);
    }
    assert_eq!(term.selection_to_string(), Some(String::from(" aaa  aaa\"")));
}

#[test]
fn semantic_selection_works() {
    let size = TermSize::new(5, 3);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut grid: Grid<Cell> = Grid::new(3, 5, 0);
    for i in 0..5 {
        for j in 0..2 {
            grid[Line(j)][Column(i)].c = 'a';
        }
    }
    grid[Line(0)][Column(0)].c = '"';
    grid[Line(0)][Column(3)].c = '"';
    grid[Line(1)][Column(2)].c = '"';
    grid[Line(0)][Column(4)].flags.insert(Flags::WRAPLINE);

    let mut escape_chars = String::from("\"");

    mem::swap(&mut term.grid, &mut grid);
    mem::swap(&mut term.config.semantic_escape_chars, &mut escape_chars);

    {
        term.selection = Some(Selection::new(
            SelectionType::Semantic,
            Point { line: Line(0), column: Column(1) },
            Side::Left,
        ));
        assert_eq!(term.selection_to_string(), Some(String::from("aa")));
    }

    {
        term.selection = Some(Selection::new(
            SelectionType::Semantic,
            Point { line: Line(0), column: Column(4) },
            Side::Left,
        ));
        assert_eq!(term.selection_to_string(), Some(String::from("aaa")));
    }

    {
        term.selection = Some(Selection::new(
            SelectionType::Semantic,
            Point { line: Line(1), column: Column(1) },
            Side::Left,
        ));
        assert_eq!(term.selection_to_string(), Some(String::from("aaa")));
    }
}

#[test]
fn line_selection_works() {
    let size = TermSize::new(5, 1);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut grid: Grid<Cell> = Grid::new(1, 5, 0);
    for i in 0..5 {
        grid[Line(0)][Column(i)].c = 'a';
    }
    grid[Line(0)][Column(0)].c = '"';
    grid[Line(0)][Column(3)].c = '"';

    mem::swap(&mut term.grid, &mut grid);

    term.selection = Some(Selection::new(
        SelectionType::Lines,
        Point { line: Line(0), column: Column(3) },
        Side::Left,
    ));
    assert_eq!(term.selection_to_string(), Some(String::from("\"aa\"a\n")));
}

#[test]
fn block_selection_works() {
    let size = TermSize::new(5, 5);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let grid = term.grid_mut();
    for i in 1..4 {
        grid[Line(i)][Column(0)].c = '"';

        for j in 1..4 {
            grid[Line(i)][Column(j)].c = 'a';
        }

        grid[Line(i)][Column(4)].c = '"';
    }
    grid[Line(2)][Column(2)].c = ' ';
    grid[Line(2)][Column(4)].flags.insert(Flags::WRAPLINE);
    grid[Line(3)][Column(4)].c = ' ';

    term.selection = Some(Selection::new(
        SelectionType::Block,
        Point { line: Line(0), column: Column(3) },
        Side::Left,
    ));

    // The same column.
    if let Some(s) = term.selection.as_mut() {
        s.update(Point { line: Line(3), column: Column(3) }, Side::Right);
    }
    assert_eq!(term.selection_to_string(), Some(String::from("\na\na\na")));

    // The first column.
    if let Some(s) = term.selection.as_mut() {
        s.update(Point { line: Line(3), column: Column(0) }, Side::Left);
    }
    assert_eq!(term.selection_to_string(), Some(String::from("\n\"aa\n\"a\n\"aa")));

    // The last column.
    if let Some(s) = term.selection.as_mut() {
        s.update(Point { line: Line(3), column: Column(4) }, Side::Right);
    }
    assert_eq!(term.selection_to_string(), Some(String::from("\na\"\na\"\na")));
}

/// Check that the grid can be serialized back and forth losslessly.
///
/// This test is in the term module as opposed to the grid since we want to
/// test this property with a T=Cell.
#[test]
#[cfg(feature = "serde")]
fn grid_serde() {
    let grid: Grid<Cell> = Grid::new(24, 80, 0);
    let serialized = serde_json::to_string(&grid).expect("ser");
    let deserialized = serde_json::from_str::<Grid<Cell>>(&serialized).expect("de");

    assert_eq!(deserialized, grid);
}

#[test]
fn input_line_drawing_character() {
    let size = TermSize::new(7, 17);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let cursor = Point::new(Line(0), Column(0));
    term.configure_charset(CharsetIndex::G0, StandardCharset::SpecialCharacterAndLineDrawing);
    term.input('a');

    assert_eq!(term.grid()[cursor].c, '▒');
}

#[test]
fn clearing_viewport_keeps_history_position() {
    let size = TermSize::new(10, 20);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Create 10 lines of scrollback.
    for _ in 0..29 {
        term.newline();
    }

    // Change the display area.
    term.scroll_display(Scroll::Top);

    assert_eq!(term.grid.display_offset(), 10);

    // Clear the viewport.
    term.clear_screen(ansi::ClearMode::All);

    assert_eq!(term.grid.display_offset(), 10);
}

#[test]
fn clearing_viewport_with_vi_mode_keeps_history_position() {
    let size = TermSize::new(10, 20);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Create 10 lines of scrollback.
    for _ in 0..29 {
        term.newline();
    }

    // Enable vi mode.
    term.toggle_vi_mode();

    // Change the display area and the vi cursor position.
    term.scroll_display(Scroll::Top);
    term.vi_mode_cursor.point = Point::new(Line(-5), Column(3));

    assert_eq!(term.grid.display_offset(), 10);

    // Clear the viewport.
    term.clear_screen(ansi::ClearMode::All);

    assert_eq!(term.grid.display_offset(), 10);
    assert_eq!(term.vi_mode_cursor.point, Point::new(Line(-5), Column(3)));
}

#[test]
fn clearing_scrollback_resets_display_offset() {
    let size = TermSize::new(10, 20);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Create 10 lines of scrollback.
    for _ in 0..29 {
        term.newline();
    }

    // Change the display area.
    term.scroll_display(Scroll::Top);

    assert_eq!(term.grid.display_offset(), 10);

    // Clear the scrollback buffer.
    term.clear_screen(ansi::ClearMode::Saved);

    assert_eq!(term.grid.display_offset(), 0);
}

#[test]
fn clearing_scrollback_sets_vi_cursor_into_viewport() {
    let size = TermSize::new(10, 20);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Create 10 lines of scrollback.
    for _ in 0..29 {
        term.newline();
    }

    // Enable vi mode.
    term.toggle_vi_mode();

    // Change the display area and the vi cursor position.
    term.scroll_display(Scroll::Top);
    term.vi_mode_cursor.point = Point::new(Line(-5), Column(3));

    assert_eq!(term.grid.display_offset(), 10);

    // Clear the scrollback buffer.
    term.clear_screen(ansi::ClearMode::Saved);

    assert_eq!(term.grid.display_offset(), 0);
    assert_eq!(term.vi_mode_cursor.point, Point::new(Line(0), Column(3)));
}

#[test]
fn clear_saved_lines() {
    let size = TermSize::new(7, 17);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Add one line of scrollback.
    term.grid.scroll_up(&(Line(0)..Line(1)), 1);

    // Clear the history.
    term.clear_screen(ansi::ClearMode::Saved);

    // Make sure that scrolling does not change the grid.
    let mut scrolled_grid = term.grid.clone();
    scrolled_grid.scroll_display(Scroll::Top);

    // Truncate grids for comparison.
    scrolled_grid.truncate();
    term.grid.truncate();

    assert_eq!(term.grid, scrolled_grid);
}

#[test]
fn vi_cursor_keep_pos_on_scrollback_buffer() {
    let size = TermSize::new(5, 10);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Create 11 lines of scrollback.
    for _ in 0..20 {
        term.newline();
    }

    // Enable vi mode.
    term.toggle_vi_mode();

    term.scroll_display(Scroll::Top);
    term.vi_mode_cursor.point.line = Line(-11);

    term.linefeed();
    assert_eq!(term.vi_mode_cursor.point.line, Line(-12));
}

#[test]
fn grow_lines_updates_active_cursor_pos() {
    let mut size = TermSize::new(100, 10);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Create 10 lines of scrollback.
    for _ in 0..19 {
        term.newline();
    }
    assert_eq!(term.history_size(), 10);
    assert_eq!(term.grid.cursor.point, Point::new(Line(9), Column(0)));

    // Increase visible lines.
    size.screen_lines = 30;
    term.resize(&size);

    assert_eq!(term.history_size(), 0);
    assert_eq!(term.grid.cursor.point, Point::new(Line(19), Column(0)));
}

#[test]
fn grow_lines_updates_inactive_cursor_pos() {
    let mut size = TermSize::new(100, 10);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Create 10 lines of scrollback.
    for _ in 0..19 {
        term.newline();
    }
    assert_eq!(term.history_size(), 10);
    assert_eq!(term.grid.cursor.point, Point::new(Line(9), Column(0)));

    // Enter alt screen.
    term.set_private_mode(NamedPrivateMode::SwapScreenAndSetRestoreCursor.into());

    // Increase visible lines.
    size.screen_lines = 30;
    term.resize(&size);

    // Leave alt screen.
    term.unset_private_mode(NamedPrivateMode::SwapScreenAndSetRestoreCursor.into());

    assert_eq!(term.history_size(), 0);
    assert_eq!(term.grid.cursor.point, Point::new(Line(19), Column(0)));
}

#[test]
fn shrink_lines_updates_active_cursor_pos() {
    let mut size = TermSize::new(100, 10);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Create 10 lines of scrollback.
    for _ in 0..19 {
        term.newline();
    }
    assert_eq!(term.history_size(), 10);
    assert_eq!(term.grid.cursor.point, Point::new(Line(9), Column(0)));

    // Increase visible lines.
    size.screen_lines = 5;
    term.resize(&size);

    assert_eq!(term.history_size(), 15);
    assert_eq!(term.grid.cursor.point, Point::new(Line(4), Column(0)));
}

#[test]
fn shrink_lines_updates_inactive_cursor_pos() {
    let mut size = TermSize::new(100, 10);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Create 10 lines of scrollback.
    for _ in 0..19 {
        term.newline();
    }
    assert_eq!(term.history_size(), 10);
    assert_eq!(term.grid.cursor.point, Point::new(Line(9), Column(0)));

    // Enter alt screen.
    term.set_private_mode(NamedPrivateMode::SwapScreenAndSetRestoreCursor.into());

    // Increase visible lines.
    size.screen_lines = 5;
    term.resize(&size);

    // Leave alt screen.
    term.unset_private_mode(NamedPrivateMode::SwapScreenAndSetRestoreCursor.into());

    assert_eq!(term.history_size(), 15);
    assert_eq!(term.grid.cursor.point, Point::new(Line(4), Column(0)));
}

#[test]
fn damage_public_usage() {
    let size = TermSize::new(10, 10);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    // Reset terminal for partial damage tests since it's initialized as fully damaged.
    term.reset_damage();

    // Test that we damage input form [`Term::input`].

    let left = term.grid.cursor.point.column.0;
    term.input('d');
    term.input('a');
    term.input('m');
    term.input('a');
    term.input('g');
    term.input('e');
    let right = term.grid.cursor.point.column.0;

    let mut damaged_lines = match term.damage() {
        TermDamage::Full => panic!("Expected partial damage, however got Full"),
        TermDamage::Partial(damaged_lines) => damaged_lines,
    };
    assert_eq!(damaged_lines.next(), Some(LineDamageBounds { line: 0, left, right }));
    assert_eq!(damaged_lines.next(), None);
    term.reset_damage();

    // Create scrollback.
    for _ in 0..20 {
        term.newline();
    }

    match term.damage() {
        TermDamage::Full => (),
        TermDamage::Partial(_) => panic!("Expected Full damage, however got Partial "),
    }
    term.reset_damage();

    term.scroll_display(Scroll::Delta(10));
    term.reset_damage();

    // No damage when scrolled into viewport.
    for idx in 0..term.columns() {
        term.goto(idx as i32, idx);
    }
    let mut damaged_lines = match term.damage() {
        TermDamage::Full => panic!("Expected partial damage, however got Full"),
        TermDamage::Partial(damaged_lines) => damaged_lines,
    };
    assert_eq!(damaged_lines.next(), None);

    // Scroll back into the viewport, so we have 2 visible lines which terminal can write
    // to.
    term.scroll_display(Scroll::Delta(-2));
    term.reset_damage();

    term.goto(0, 0);
    term.goto(1, 0);
    term.goto(2, 0);
    let display_offset = term.grid().display_offset();
    let mut damaged_lines = match term.damage() {
        TermDamage::Full => panic!("Expected partial damage, however got Full"),
        TermDamage::Partial(damaged_lines) => damaged_lines,
    };
    assert_eq!(
        damaged_lines.next(),
        Some(LineDamageBounds { line: display_offset, left: 0, right: 0 })
    );
    assert_eq!(
        damaged_lines.next(),
        Some(LineDamageBounds { line: display_offset + 1, left: 0, right: 0 })
    );
    assert_eq!(damaged_lines.next(), None);
}

#[test]
fn damage_cursor_movements() {
    let size = TermSize::new(10, 10);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let num_cols = term.columns();
    // Reset terminal for partial damage tests since it's initialized as fully damaged.
    term.reset_damage();

    term.goto(1, 1);

    // NOTE While we can use `[Term::damage]` to access terminal damage information, in the
    // following tests we will be accessing `term.damage.lines` directly to avoid adding extra
    // damage information (like cursor and Vi cursor), which we're not testing.

    assert_eq!(term.damage.lines[0], LineDamageBounds { line: 0, left: 0, right: 0 });
    assert_eq!(term.damage.lines[1], LineDamageBounds { line: 1, left: 1, right: 1 });
    term.damage.reset(num_cols);

    term.move_forward(3);
    assert_eq!(term.damage.lines[1], LineDamageBounds { line: 1, left: 1, right: 4 });
    term.damage.reset(num_cols);

    term.move_backward(8);
    assert_eq!(term.damage.lines[1], LineDamageBounds { line: 1, left: 0, right: 4 });
    term.goto(5, 5);
    term.damage.reset(num_cols);

    term.backspace();
    term.backspace();
    assert_eq!(term.damage.lines[5], LineDamageBounds { line: 5, left: 3, right: 5 });
    term.damage.reset(num_cols);

    term.move_up(1);
    assert_eq!(term.damage.lines[5], LineDamageBounds { line: 5, left: 3, right: 3 });
    assert_eq!(term.damage.lines[4], LineDamageBounds { line: 4, left: 3, right: 3 });
    term.damage.reset(num_cols);

    term.move_down(1);
    term.move_down(1);
    assert_eq!(term.damage.lines[4], LineDamageBounds { line: 4, left: 3, right: 3 });
    assert_eq!(term.damage.lines[5], LineDamageBounds { line: 5, left: 3, right: 3 });
    assert_eq!(term.damage.lines[6], LineDamageBounds { line: 6, left: 3, right: 3 });
    term.damage.reset(num_cols);

    term.wrapline();
    assert_eq!(term.damage.lines[6], LineDamageBounds { line: 6, left: 3, right: 3 });
    assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right: 0 });
    term.move_forward(3);
    term.move_up(1);
    term.damage.reset(num_cols);

    term.linefeed();
    assert_eq!(term.damage.lines[6], LineDamageBounds { line: 6, left: 3, right: 3 });
    assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 3, right: 3 });
    term.damage.reset(num_cols);

    term.carriage_return();
    assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right: 3 });
    term.damage.reset(num_cols);

    term.erase_chars(5);
    assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right: 5 });
    term.damage.reset(num_cols);

    term.delete_chars(3);
    let right = term.columns() - 1;
    assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right });
    term.move_forward(term.columns());
    term.damage.reset(num_cols);

    term.move_backward_tabs(1);
    assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 8, right });
    term.save_cursor_position();
    term.goto(1, 1);
    term.damage.reset(num_cols);

    term.restore_cursor_position();
    assert_eq!(term.damage.lines[1], LineDamageBounds { line: 1, left: 1, right: 1 });
    assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 8, right: 8 });
    term.damage.reset(num_cols);

    term.clear_line(ansi::LineClearMode::All);
    assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right });
    term.damage.reset(num_cols);

    term.clear_line(ansi::LineClearMode::Left);
    assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 0, right: 8 });
    term.damage.reset(num_cols);

    term.clear_line(ansi::LineClearMode::Right);
    assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 8, right });
    term.damage.reset(num_cols);

    term.reverse_index();
    assert_eq!(term.damage.lines[7], LineDamageBounds { line: 7, left: 8, right: 8 });
    assert_eq!(term.damage.lines[6], LineDamageBounds { line: 6, left: 8, right: 8 });
}

#[test]
fn full_damage() {
    let size = TermSize::new(100, 10);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    assert!(term.damage.full);
    for _ in 0..20 {
        term.newline();
    }
    term.reset_damage();

    term.clear_screen(ansi::ClearMode::Above);
    assert!(term.damage.full);
    term.reset_damage();

    term.scroll_display(Scroll::Top);
    assert!(term.damage.full);
    term.reset_damage();

    // Sequential call to scroll display without doing anything shouldn't damage.
    term.scroll_display(Scroll::Top);
    assert!(!term.damage.full);
    term.reset_damage();

    term.set_options(Config::default());
    assert!(term.damage.full);
    term.reset_damage();

    term.scroll_down_relative(Line(5), 2);
    assert!(term.damage.full);
    term.reset_damage();

    term.scroll_up_relative(Line(3), 2);
    assert!(term.damage.full);
    term.reset_damage();

    term.deccolm();
    assert!(term.damage.full);
    term.reset_damage();

    term.decaln();
    assert!(term.damage.full);
    term.reset_damage();

    term.set_mode(NamedMode::Insert.into());
    // Just setting `Insert` mode shouldn't mark terminal as damaged.
    assert!(!term.damage.full);
    term.reset_damage();

    let color_index = 257;
    term.set_color(color_index, Rgb::default());
    assert!(term.damage.full);
    term.reset_damage();

    // Setting the same color once again shouldn't trigger full damage.
    term.set_color(color_index, Rgb::default());
    assert!(!term.damage.full);

    term.reset_color(color_index);
    assert!(term.damage.full);
    term.reset_damage();

    // We shouldn't trigger fully damage when cursor gets update.
    term.set_color(NamedColor::Cursor as usize, Rgb::default());
    assert!(!term.damage.full);

    // However requesting terminal damage should mark terminal as fully damaged in `Insert`
    // mode.
    let _ = term.damage();
    assert!(term.damage.full);
    term.reset_damage();

    term.unset_mode(NamedMode::Insert.into());
    assert!(term.damage.full);
    term.reset_damage();

    // Keep this as a last check, so we don't have to deal with restoring from alt-screen.
    term.swap_alt();
    assert!(term.damage.full);
    term.reset_damage();

    let size = TermSize::new(10, 10);
    term.resize(&size);
    assert!(term.damage.full);
}

#[test]
fn window_title() {
    let size = TermSize::new(7, 17);
    let mut term = Term::new(Config::default(), &size, VoidListener);

    // Title None by default.
    assert_eq!(term.title, None);

    // Title can be set.
    term.set_title(Some("Test".into()));
    assert_eq!(term.title, Some("Test".into()));

    // Title can be pushed onto stack.
    term.push_title();
    term.set_title(Some("Next".into()));
    assert_eq!(term.title, Some("Next".into()));
    assert_eq!(term.title_stack.first().unwrap(), &Some("Test".into()));

    // Title can be popped from stack and set as the window title.
    term.pop_title();
    assert_eq!(term.title, Some("Test".into()));
    assert!(term.title_stack.is_empty());

    // Title stack doesn't grow infinitely.
    for _ in 0..4097 {
        term.push_title();
    }
    assert_eq!(term.title_stack.len(), 4096);

    // Title and title stack reset when terminal state is reset.
    term.push_title();
    term.reset_state();
    assert_eq!(term.title, None);
    assert!(term.title_stack.is_empty());

    // Title stack pops back to default.
    term.title = None;
    term.push_title();
    term.set_title(Some("Test".into()));
    term.pop_title();
    assert_eq!(term.title, None);

    // Title can be reset to default.
    term.title = Some("Test".into());
    term.set_title(None);
    assert_eq!(term.title, None);
}

#[test]
fn kitty_keyboard_mode_stack_is_bounded() {
    let size = TermSize::new(1, 1);
    let config = Config { kitty_keyboard: true, ..Config::default() };
    let mut term = Term::new(config, &size, VoidListener);

    for _ in 0..=KEYBOARD_MODE_STACK_MAX_DEPTH {
        term.push_keyboard_mode(KeyboardModes::DISAMBIGUATE_ESC_CODES);
    }

    assert_eq!(term.keyboard_mode_stack.len(), KEYBOARD_MODE_STACK_MAX_DEPTH);
    assert!(term.title_stack.is_empty());
}

#[test]
fn leading_wide_spacer_copies_glyph_from_next_line() {
    let size = TermSize::new(2, 2);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    term.grid[Line(0)][Column(0)].c = 'a';
    term.grid[Line(0)][Column(1)].flags.insert(Flags::LEADING_WIDE_CHAR_SPACER | Flags::WRAPLINE);
    term.grid[Line(1)][Column(0)].c = '界';
    term.grid[Line(1)][Column(0)].flags.insert(Flags::WIDE_CHAR);
    term.grid[Line(1)][Column(1)].flags.insert(Flags::WIDE_CHAR_SPACER);

    let text =
        term.bounds_to_string(Point::new(Line(0), Column(0)), Point::new(Line(0), Column(1)));
    assert_eq!(text, "a界");
}

#[test]
fn delete_chars_is_bounded_by_remaining_columns() {
    let size = TermSize::new(5, 1);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    for (column, character) in "abcde".chars().enumerate() {
        term.grid[Line(0)][Column(column)].c = character;
    }
    term.grid.cursor.point.column = Column(3);

    term.delete_chars(usize::MAX);

    let row = &term.grid[Line(0)];
    assert_eq!(row[Column(0)].c, 'a');
    assert_eq!(row[Column(1)].c, 'b');
    assert_eq!(row[Column(2)].c, 'c');
    assert_eq!(row[Column(3)].c, ' ');
    assert_eq!(row[Column(4)].c, ' ');
}

#[test]
fn clear_above_includes_first_row() {
    let size = TermSize::new(2, 2);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    term.grid[Line(0)][Column(0)].c = 'x';
    term.grid.cursor.point = Point::new(Line(1), Column(0));

    term.clear_screen(ansi::ClearMode::Above);

    assert_eq!(term.grid[Line(0)][Column(0)].c, ' ');
}

#[test]
fn reporting_blinking_cursor_does_not_set_an_override() {
    let size = TermSize::new(2, 1);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    assert!(term.cursor_style.is_none());

    term.report_private_mode(PrivateMode::Named(NamedPrivateMode::BlinkingCursor));

    assert!(term.cursor_style.is_none());
}

#[test]
fn scrolling_region_is_valid_after_clamping() {
    let size = TermSize::new(2, 3);
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let original = term.scroll_region.clone();

    term.set_scrolling_region(99, Some(100));
    assert_eq!(term.scroll_region, original);

    term.set_scrolling_region(2, Some(99));
    assert_eq!(term.scroll_region, Line(1)..Line(3));
}

#[test]
fn parse_cargo_version() {
    assert!(version_number(env!("CARGO_PKG_VERSION")) >= 10_01);
    assert_eq!(version_number("0.0.1-dev"), 1);
    assert_eq!(version_number("0.1.2-dev"), 1_02);
    assert_eq!(version_number("1.2.3-dev"), 1_02_03);
    assert_eq!(version_number("999.99.99"), 9_99_99_99);
}
