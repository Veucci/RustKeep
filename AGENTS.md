# AGENTS.md

Rules for any agent or contributor working on this repository. These rules apply project-wide: the backend (`src/`) and the Leptos frontend (`web/`). They are not optional; a change that breaks one of them is not finished.

## 1. No `unsafe`, memory safety only

- Never use `unsafe` in any form: no `unsafe` blocks, `unsafe fn`, `unsafe impl`, or `unsafe trait`.
- Do not work around this through indirection: no raw pointer dereferencing, `std::mem::transmute`, `MaybeUninit::assume_init`, `from_raw_parts`, `get_unchecked`, `unwrap_unchecked`, FFI declarations (`extern "C"`), or `#[no_mangle]`.
- Do not add a dependency whose main purpose is to expose unsafe operations to this codebase. Well-established crates that use `unsafe` internally behind a safe API (tokio, axum, leptos, etc.) are fine.
- If a problem seems to require `unsafe`, solve it with a safe alternative (safe std APIs, owned data, `Arc`/`Mutex`, a safe crate) or stop and ask.

## 2. No code comments

- Do not leave comments anywhere in the code: no `//`, `/* */`, `///`, `//!`, HTML comments, or CSS comments.
- Code must explain itself through clear names for functions, variables, and types, small focused functions, and straightforward control flow.
- If a piece of code feels like it needs a comment, rename or restructure it until it does not.
- Do not leave commented-out code, TODO/FIXME notes, or section dividers.

## 3. No custom CSS unless unavoidable

- Style the UI with the shadcn components already in the project (`web/src/components/ui/`, rust-ui / `leptos_ui`) and Tailwind utility classes.
- Do not add new rules to `web/style/tailwind.css` or any other stylesheet, and do not use inline `style` attributes, when a shadcn component, a component variant, or Tailwind utilities can achieve the result.
- If a needed component does not exist yet, add the shadcn (rust-ui) version of it to `web/src/components/ui/` instead of building a hand-styled one.
- Custom CSS is acceptable only when there is truly no shadcn/Tailwind way to do it. Keep it minimal and use the existing theme tokens (CSS variables) instead of hard-coded colors.

## 4. No native browser UI components

- Do not use the browser's built-in UI widgets: no native date/time pickers (`<input type="date">`, `type="time"`, `type="datetime-local"`), native color pickers, `<dialog>`, `window.alert`, `window.confirm`, `window.prompt`, or unstyled native `<select>` dropdowns.
- Use the shadcn equivalents from `web/src/components/ui/` (Dialog, Alert Dialog, Date Picker / Calendar, Select, Popover, etc.).
- If the required shadcn component is not in the project yet, add it to `web/src/components/ui/` first and then use it, so every screen shares the same look and behavior.

## 5. English only in code

- Everything in the codebase must be in English: identifiers, string literals, UI text, log and error messages, test names, file names, and commit messages.
- No other language may appear in code under any circumstances, including Turkish, even temporarily.
- Data stored by users is not covered by this rule; only what is written in the source is.

## 6. No data loss on schema or data changes

- Schema migrations run automatically on startup (`src/migrate.rs`). When a table definition changes, the table is rebuilt and only columns present in both the old and the new definition are copied. Any column that is removed or renamed is **dropped with its data**.
- Therefore never ship a change that loses existing data:
  - Do not rename a column directly. Add the new column, copy the values over, and only remove the old one once the data is safely moved.
  - Do not remove a column or table that still holds data the app needs.
  - New `NOT NULL` columns must have a `DEFAULT` so existing rows can be copied.
  - Do not tighten constraints (`NOT NULL`, `UNIQUE`, `CHECK`, foreign keys) in a way existing rows would violate; fix the data first.
  - Changes to stored formats (JSON shapes, enum values, encrypted payloads, file paths) must stay readable for already stored records or convert them during migration.
- Before changing any schema, think through what the automatic migration will do to existing rows and make sure every record survives.
- When in doubt, keep the old data and ask instead of deleting it.
