use super::*;
use crate::native_app::app_chrome::palette::SELECTION_FLASH_FILL;
use radiant::gui::types::Point;
use radiant::prelude::{self as ui, IntoView};
use radiant::runtime::{SurfacePaintPlan, UiSurface};
use radiant::widgets::{
    DragHandleMessage, InteractiveRowMessage, PointerButton, PointerModifiers, WidgetInput,
    WidgetOutput, stable_widget_id,
};

const TEST_INPUT_ID: u64 = 99_501;

fn message_from(surface: &UiSurface<GuiMessage>, output: Option<WidgetOutput>) -> GuiMessage {
    surface
        .dispatch_widget_output(TEST_INPUT_ID, output.expect("expected widget output"))
        .expect("expected sample file message")
}

fn sample_hit_target(
    selected: bool,
    drag_active: bool,
    drag_source: bool,
    cached: bool,
) -> UiSurface<GuiMessage> {
    sample_hit_target_with_model(SampleFileHitTargetModel {
        file_id: "sample.wav",
        selected,
        focused: false,
        focus_alpha: 0,
        selection_flash: false,
        copy_flash: false,
        protected_source_error_flash: false,
        cut_pending: false,
        drag_active,
        drag_source,
        cached,
        missing: false,
        hit_path: String::from("sample.wav"),
        help_tooltips_enabled: false,
    })
}

fn sample_hit_target_with_focus(
    selected: bool,
    focused: bool,
    cached: bool,
) -> UiSurface<GuiMessage> {
    sample_hit_target_with_focus_and_explicit_selection(selected, focused, cached)
}

fn sample_hit_target_with_focus_and_explicit_selection(
    selected: bool,
    focused: bool,
    cached: bool,
) -> UiSurface<GuiMessage> {
    sample_hit_target_with_model(SampleFileHitTargetModel {
        file_id: "sample.wav",
        selected,
        focused,
        focus_alpha: if focused { u8::MAX } else { 0 },
        selection_flash: false,
        copy_flash: false,
        protected_source_error_flash: false,
        cut_pending: false,
        drag_active: false,
        drag_source: false,
        cached,
        missing: false,
        hit_path: String::from("sample.wav"),
        help_tooltips_enabled: false,
    })
}

fn sample_hit_target_with_copy_flash(selected: bool, cached: bool) -> UiSurface<GuiMessage> {
    sample_hit_target_with_model(SampleFileHitTargetModel {
        file_id: "sample.wav",
        selected,
        focused: false,
        focus_alpha: 0,
        selection_flash: false,
        copy_flash: true,
        protected_source_error_flash: false,
        cut_pending: false,
        drag_active: false,
        drag_source: false,
        cached,
        missing: false,
        hit_path: String::from("sample.wav"),
        help_tooltips_enabled: false,
    })
}

fn sample_hit_target_with_selection_flash(cached: bool) -> UiSurface<GuiMessage> {
    sample_hit_target_with_model(SampleFileHitTargetModel {
        file_id: "sample.wav",
        selected: true,
        focused: false,
        focus_alpha: 0,
        selection_flash: true,
        copy_flash: false,
        protected_source_error_flash: false,
        cut_pending: false,
        drag_active: false,
        drag_source: false,
        cached,
        missing: false,
        hit_path: String::from("sample.wav"),
        help_tooltips_enabled: false,
    })
}

fn sample_hit_target_with_cut_pending(selected: bool, cached: bool) -> UiSurface<GuiMessage> {
    sample_hit_target_with_model(SampleFileHitTargetModel {
        file_id: "sample.wav",
        selected,
        focused: false,
        focus_alpha: 0,
        selection_flash: false,
        copy_flash: false,
        protected_source_error_flash: false,
        cut_pending: true,
        drag_active: false,
        drag_source: false,
        cached,
        missing: false,
        hit_path: String::from("sample.wav"),
        help_tooltips_enabled: false,
    })
}

fn sample_hit_target_with_model(model: SampleFileHitTargetModel<'_>) -> UiSurface<GuiMessage> {
    sample_file_hit_target_for_tests(ui::empty(), model, TEST_INPUT_ID)
        .size(120.0, 22.0)
        .into_surface()
}

fn dispatch(
    surface: &mut UiSurface<GuiMessage>,
    bounds: ui::Rect,
    input: WidgetInput,
) -> Option<WidgetOutput> {
    surface.dispatch_widget_input(TEST_INPUT_ID, bounds, input)
}

fn synchronize_widget_from_previous(
    current: &mut UiSurface<GuiMessage>,
    previous: &UiSurface<GuiMessage>,
) {
    let previous = previous
        .find_widget(TEST_INPUT_ID)
        .expect("previous sample row widget")
        .widget();
    let current = current
        .find_widget_mut(TEST_INPUT_ID)
        .expect("current sample row widget")
        .widget_mut();
    current.synchronize_from_previous(previous);
}

fn sample_widget_plan(surface: &UiSurface<GuiMessage>, bounds: ui::Rect) -> SurfacePaintPlan {
    surface
        .find_widget(TEST_INPUT_ID)
        .expect("sample row widget")
        .widget()
        .paint_plan_with_defaults(bounds)
}

fn sample_widget_is_hovered(surface: &UiSurface<GuiMessage>) -> bool {
    surface
        .find_widget(TEST_INPUT_ID)
        .expect("sample row widget")
        .widget()
        .common()
        .is_hovered()
}

#[test]
/// Verifies production rows derive input identity from the same stable row key used by the row subtree.
fn production_hit_target_derives_stable_input_identity_from_sample_row_key() {
    let row_key = identity::retained_sample_row_key("sample.wav");
    let input_id = stable_widget_id(identity::RETAINED_SAMPLE_ROW_INPUT_SCOPE, row_key.as_str());
    let message = sample_file_hit_target(
        ui::empty(),
        SampleFileHitTargetModel {
            file_id: "sample.wav",
            selected: false,
            focused: false,
            focus_alpha: 0,
            selection_flash: false,
            copy_flash: false,
            protected_source_error_flash: false,
            cut_pending: false,
            drag_active: false,
            drag_source: false,
            cached: false,
            missing: false,
            hit_path: String::from("sample.wav"),
            help_tooltips_enabled: false,
        },
    )
    .view_dispatch_widget_output(
        input_id,
        WidgetOutput::typed(InteractiveRowMessage::DoubleActivate),
    );

    assert_eq!(
        message,
        Some(GuiMessage::SelectSampleWithModifiers {
            path: String::from("sample.wav"),
            modifiers: PointerModifiers::default(),
        })
    );
}

fn sample_widget_is_pressed(surface: &UiSurface<GuiMessage>) -> bool {
    surface
        .find_widget(TEST_INPUT_ID)
        .expect("sample row widget")
        .widget()
        .common()
        .is_pressed()
}

fn sample_widget_accepts_pointer_move(surface: &UiSurface<GuiMessage>) -> bool {
    surface
        .find_widget(TEST_INPUT_ID)
        .expect("sample row widget")
        .widget()
        .accepts_pointer_move()
}

fn set_sample_widget_hovered(surface: &mut UiSurface<GuiMessage>, hovered: bool) {
    surface
        .find_widget_mut(TEST_INPUT_ID)
        .expect("sample row widget")
        .widget_mut()
        .common_mut()
        .state
        .hovered = hovered;
}

fn paints_hover_fill(plan: &SurfacePaintPlan) -> bool {
    let hover = sample_row_palette_for_tests()
        .hovered
        .expect("dense-row hover fill");
    plan.fill_rects().any(|fill| fill.color == hover)
}

fn selected_fill() -> ui::Rgba8 {
    sample_row_palette_for_tests()
        .selected
        .expect("dense-row selected fill")
}

fn selected_hover_fill() -> ui::Rgba8 {
    sample_row_palette_for_tests()
        .selected_hovered
        .expect("dense-row selected hover fill")
}

fn paints_selection_marker(plan: &SurfacePaintPlan, bounds: ui::Rect) -> bool {
    let marker = selected_row_marker();
    plan.fill_rects().any(|fill| {
        fill.rect.min.x >= bounds.min.x
            && fill.rect.max.x <= bounds.max.x
            && fill.rect.width() == marker.parts.width
            && fill.rect.height() == bounds.height()
            && fill.color == marker.color
    })
}

fn paints_focus_marker(plan: &SurfacePaintPlan, bounds: ui::Rect) -> bool {
    let marker = crate::native_app::app_chrome::palette::focused_row_marker();
    plan.fill_rects().any(|fill| {
        fill.rect.min.x == bounds.min.x
            && fill.rect.width() == marker.parts.width
            && fill.color == marker.color
    })
}

fn paints_hover_trailing_marker(plan: &SurfacePaintPlan, bounds: ui::Rect) -> bool {
    let marker = crate::native_app::app_chrome::palette::hovered_row_trailing_marker();
    plan.fill_rects().any(|fill| {
        fill.rect.max.x == bounds.max.x
            && fill.rect.width() == marker.parts.width
            && fill.color == marker.color
    })
}

#[test]
/// Verifies selected rows keep the existing fill and leading marker without adding focus chrome.
fn selected_rows_paint_selection_fill_and_marker_without_focus_marker() {
    let bounds = ui::Rect::from_xy_size(10.0, 20.0, 120.0, 22.0);
    let target = sample_hit_target_with_focus(true, false, false);
    let plan = sample_widget_plan(&target, bounds);

    assert!(
        plan.fill_rects().any(|fill| fill.color == selected_fill()),
        "selected rows should keep the selected fill"
    );
    assert!(
        paints_selection_marker(&plan, bounds),
        "selected rows should keep the leading selected marker"
    );
    assert!(
        !paints_focus_marker(&plan, bounds),
        "selection alone should not paint the focused-row marker"
    );
}

#[test]
/// Verifies the ordinary current row does not hide the explicit X-selection affordance.
fn implicit_focused_selection_does_not_paint_explicit_selection_chrome() {
    let bounds = ui::Rect::from_xy_size(10.0, 20.0, 120.0, 22.0);
    let target = sample_hit_target_with_focus_and_explicit_selection(false, true, false);
    let plan = sample_widget_plan(&target, bounds);

    assert!(
        paints_focus_marker(&plan, bounds),
        "the current sample should keep the focused-row marker"
    );
    assert!(
        !plan.fill_rects().any(|fill| fill.color == selected_fill()),
        "implicit current-row selection should not already paint the marked-selection fill"
    );
    assert!(
        !paints_selection_marker(&plan, bounds),
        "implicit current-row selection should not already paint the marked-selection marker"
    );
}

#[test]
/// Verifies focus can move independently from the selected set.
fn focused_rows_paint_focus_marker_without_selection_fill_or_marker() {
    let bounds = ui::Rect::from_xy_size(10.0, 20.0, 120.0, 22.0);
    let target = sample_hit_target_with_focus(false, true, false);
    let plan = sample_widget_plan(&target, bounds);

    assert!(
        paints_focus_marker(&plan, bounds),
        "focused rows should paint a crisp leading marker"
    );
    assert!(
        !plan.fill_rects().any(|fill| fill.color == selected_fill()),
        "focused-only rows should not borrow the selected fill"
    );
    assert!(
        !paints_selection_marker(&plan, bounds),
        "focused-only rows should not borrow the selected marker"
    );
}

#[test]
/// Verifies combined selected and focused state layers both visual treatments.
fn selected_focused_rows_paint_selection_and_focus_together() {
    let bounds = ui::Rect::from_xy_size(10.0, 20.0, 120.0, 22.0);
    let target = sample_hit_target_with_focus(true, true, false);
    let plan = sample_widget_plan(&target, bounds);

    assert!(
        plan.fill_rects().any(|fill| fill.color == selected_fill()),
        "selected + focused rows should keep the selected fill"
    );
    assert!(
        paints_selection_marker(&plan, bounds),
        "selected + focused rows should keep the selected marker"
    );
    assert!(
        paints_focus_marker(&plan, bounds),
        "selected + focused rows should also paint the focus marker"
    );
}

#[test]
/// Verifies mouse-down borrows focus chrome without replacing selection with an opaque bar.
fn selected_pointer_press_uses_focus_marker_until_release() {
    let bounds = ui::Rect::from_xy_size(10.0, 20.0, 120.0, 22.0);
    let mut target = sample_hit_target_with_focus(true, false, false);

    dispatch(
        &mut target,
        bounds,
        WidgetInput::primary_press(Point::new(34.0, 28.0)),
    );
    let pressed = sample_widget_plan(&target, bounds);

    assert!(paints_focus_marker(&pressed, bounds));
    assert!(
        pressed
            .fill_rects()
            .any(|fill| fill.color == selected_fill())
    );
    assert!(paints_selection_marker(&pressed, bounds));

    dispatch(
        &mut target,
        bounds,
        WidgetInput::primary_release(Point::new(34.0, 28.0)),
    );
    let released = sample_widget_plan(&target, bounds);

    assert!(!paints_focus_marker(&released, bounds));
    assert!(
        released
            .fill_rects()
            .any(|fill| fill.color == selected_hover_fill())
    );
    assert!(paints_selection_marker(&released, bounds));
}

#[test]
/// Verifies hover feedback does not mask the focus marker.
fn focused_hover_rows_keep_focus_marker() {
    let bounds = ui::Rect::from_size(120.0, 22.0);
    let mut target = sample_hit_target_with_focus(false, true, false);
    dispatch(
        &mut target,
        bounds,
        WidgetInput::pointer_move(Point::new(34.0, 8.0)),
    );

    let plan = sample_widget_plan(&target, bounds);

    assert!(paints_hover_fill(&plan));
    assert!(paints_hover_trailing_marker(&plan, bounds));
    assert!(
        paints_focus_marker(&plan, bounds),
        "hover feedback should not erase the focused-row marker"
    );
}

#[test]
/// Verifies copied file rows flash without pretending to be selected.
fn copied_rows_paint_flash_fill_without_selection_marker() {
    let bounds = ui::Rect::from_xy_size(10.0, 20.0, 120.0, 22.0);
    let target = sample_hit_target_with_copy_flash(false, true);
    let plan = sample_widget_plan(&target, bounds);

    assert!(
        plan.fill_rects().any(|fill| fill.color == COPY_FLASH_FILL),
        "copied rows should paint the transient copy flash fill"
    );
    assert!(
        !plan.fill_rects().any(
            |fill| fill.rect.min.x == bounds.min.x && fill.color == selected_row_marker().color
        ),
        "copy flash must not add the selection marker"
    );
    assert!(
        !plan.fill_rects().any(|fill| fill.color == CACHED_MARKER),
        "copy flash fill should not be interrupted by the cached marker"
    );
}

#[test]
fn x_marked_rows_paint_a_stronger_selection_flash() {
    let bounds = ui::Rect::from_xy_size(10.0, 20.0, 120.0, 22.0);
    let target = sample_hit_target_with_selection_flash(true);
    let plan = sample_widget_plan(&target, bounds);

    assert!(
        plan.fill_rects()
            .any(|fill| fill.color == SELECTION_FLASH_FILL),
        "x-marked rows should paint the transient accent flash"
    );
    assert!(
        plan.fill_rects().any(
            |fill| fill.rect.min.x == bounds.min.x && fill.color == selected_row_marker().color
        ),
        "the flash must preserve the row's persistent selection marker"
    );
    assert!(
        !plan.fill_rects().any(|fill| fill.color == CACHED_MARKER),
        "the flash should not be interrupted by the cached marker"
    );
}

#[test]
/// Verifies retained rows repaint when copy flash begins after an ordinary frame.
fn retained_rows_repaint_copy_flash_after_refresh() {
    let bounds = ui::Rect::from_xy_size(10.0, 20.0, 120.0, 22.0);
    let previous = sample_hit_target(false, false, false, true);
    let mut refreshed = sample_hit_target_with_copy_flash(false, true);

    synchronize_widget_from_previous(&mut refreshed, &previous);
    let plan = sample_widget_plan(&refreshed, bounds);

    assert!(
        plan.fill_rects().any(|fill| fill.color == COPY_FLASH_FILL),
        "copy flash should survive retained row synchronization"
    );
}

#[test]
/// Verifies cut rows keep a persistent move-pending treatment until paste completes.
fn cut_pending_rows_paint_move_pending_fill_and_marker() {
    let bounds = ui::Rect::from_xy_size(10.0, 20.0, 120.0, 22.0);
    let target = sample_hit_target_with_cut_pending(false, true);
    let plan = sample_widget_plan(&target, bounds);

    assert!(
        plan.fill_rects().any(|fill| fill.color == CUT_PENDING_FILL),
        "cut rows should paint a persistent move-pending fill"
    );
    assert!(
        plan.fill_rects()
            .any(|fill| fill.color == CUT_PENDING_MARKER),
        "cut rows should keep a leading move-pending marker"
    );
    assert!(
        !plan.fill_rects().any(|fill| fill.color == CACHED_MARKER),
        "cut rows should not be interrupted by the cached marker"
    );
}

#[test]
/// Verifies retained refreshes do not duplicate runtime drag-preview motion.
fn active_drag_uses_runtime_preview_after_widget_refresh() {
    let bounds = ui::Rect::from_size(120.0, 22.0);
    let mut first = sample_hit_target(false, false, false, false);
    dispatch(
        &mut first,
        bounds,
        WidgetInput::primary_press(Point::new(6.0, 6.0)),
    );
    let output = dispatch(
        &mut first,
        bounds,
        WidgetInput::pointer_move(Point::new(16.0, 7.0)),
    );
    assert_eq!(
        message_from(&first, output),
        GuiMessage::DragSampleFile {
            path: String::from("sample.wav"),
            drag: DragHandleMessage::started_from(Point::new(6.0, 6.0), Point::new(16.0, 7.0),),
        }
    );

    let mut refreshed = sample_hit_target(false, true, true, false);
    synchronize_widget_from_previous(&mut refreshed, &first);
    assert!(
        dispatch(
            &mut refreshed,
            bounds,
            WidgetInput::pointer_move(Point::new(34.0, 8.0)),
        )
        .is_none(),
        "runtime drag preview already tracks pointer movement without app refresh"
    );
}

#[test]
/// Verifies refreshed drag-source rows can still end the drag sequence.
fn active_drag_source_does_not_depend_on_retained_pressed_state() {
    let bounds = ui::Rect::from_size(120.0, 22.0);
    let mut refreshed = sample_hit_target(false, true, true, false);

    assert!(
        dispatch(
            &mut refreshed,
            bounds,
            WidgetInput::pointer_move(Point::new(34.0, 8.0)),
        )
        .is_none(),
        "active drag moves are runtime-local and should not require retained pressed state"
    );
    let output = dispatch(
        &mut refreshed,
        bounds,
        WidgetInput::primary_release(Point::new(220.0, 90.0)),
    );
    assert_eq!(
        message_from(&refreshed, output),
        GuiMessage::DragSampleFile {
            path: String::from("sample.wav"),
            drag: DragHandleMessage::ended(Point::new(220.0, 90.0)),
        }
    );
}

#[test]
/// Verifies non-source rows clear hover while another sample row is dragged.
fn active_drag_non_source_rows_do_not_keep_hover_highlight() {
    let bounds = ui::Rect::from_size(120.0, 22.0);
    let mut target = sample_hit_target(false, true, false, false);
    set_sample_widget_hovered(&mut target, true);

    assert!(
        dispatch(
            &mut target,
            bounds,
            WidgetInput::pointer_move(Point::new(34.0, 8.0)),
        )
        .is_none()
    );
    assert!(
        !sample_widget_is_hovered(&target),
        "sample rows should not retain hover while another file is being dragged"
    );

    let plan = sample_widget_plan(&target, bounds);
    assert!(
        !paints_hover_fill(&plan),
        "non-source rows should not paint hover highlights during active file drags"
    );
}

#[test]
/// Verifies ordinary sample rows do not request stable pointer-motion routing.
fn idle_rows_do_not_request_stable_pointer_moves() {
    let target = sample_hit_target(false, false, false, false);

    assert!(
        !sample_widget_accepts_pointer_move(&target),
        "idle sample rows should update hover on enter/leave without routing every stable move"
    );
}

#[test]
/// Verifies pressed sample rows keep motion so drags can start reliably.
fn pressed_rows_request_pointer_moves_for_drag_start() {
    let bounds = ui::Rect::from_size(120.0, 22.0);
    let mut target = sample_hit_target(false, false, false, false);
    dispatch(
        &mut target,
        bounds,
        WidgetInput::primary_press(Point::new(34.0, 8.0)),
    );

    assert!(
        sample_widget_accepts_pointer_move(&target),
        "pressed sample rows need pointer motion to detect drag start"
    );
}

#[test]
/// Verifies active sample drags keep motion routing for source and non-source rows.
fn active_drag_rows_request_pointer_moves() {
    let source = sample_hit_target(false, true, true, false);
    let non_source = sample_hit_target(false, true, false, false);

    assert!(
        sample_widget_accepts_pointer_move(&source),
        "active drag source rows still need release and drag-session motion routing"
    );
    assert!(
        sample_widget_accepts_pointer_move(&non_source),
        "non-source rows need drag-active motion routing to clear stale hover"
    );
}

#[test]
/// Verifies retained refreshes keep hover for the current row owner.
fn hover_state_survives_retained_widget_refresh() {
    let bounds = ui::Rect::from_size(120.0, 22.0);
    let mut previous = sample_hit_target(false, false, false, false);
    dispatch(
        &mut previous,
        bounds,
        WidgetInput::pointer_move(Point::new(34.0, 8.0)),
    );
    assert!(sample_widget_is_hovered(&previous));

    let mut refreshed = sample_hit_target(false, false, false, false);
    synchronize_widget_from_previous(&mut refreshed, &previous);

    assert!(
        sample_widget_is_hovered(&refreshed),
        "sample row hover paint must survive retained projections for the current hover owner"
    );
    let plan = sample_widget_plan(&refreshed, bounds);
    assert!(
        paints_hover_fill(&plan),
        "refreshed current-hover rows should keep painting hover highlights"
    );
}

#[test]
/// Verifies unselected hover paint remains visually neutral.
fn hover_fill_is_neutral_not_selection_red() {
    let bounds = ui::Rect::from_size(120.0, 22.0);
    let mut target = sample_hit_target(false, false, false, false);
    dispatch(
        &mut target,
        bounds,
        WidgetInput::pointer_move(Point::new(34.0, 8.0)),
    );

    let plan = sample_widget_plan(&target, bounds);

    assert!(paints_hover_fill(&plan));
    assert!(paints_hover_trailing_marker(&plan, bounds));
    assert!(!plan.fill_rects().any(|fill| fill.color == selected_fill()));
}

#[test]
/// Verifies row activation preserves primary-release modifier state.
fn primary_activation_preserves_release_modifiers() {
    let bounds = ui::Rect::from_size(120.0, 22.0);
    let mut target = sample_hit_target(false, false, false, false);
    let modifiers = PointerModifiers {
        shift: true,
        ..PointerModifiers::default()
    };

    dispatch(
        &mut target,
        bounds,
        WidgetInput::primary_press(Point::new(34.0, 8.0)),
    );

    let output = dispatch(
        &mut target,
        bounds,
        WidgetInput::pointer_release(Point::new(34.0, 8.0), PointerButton::Primary, modifiers),
    );
    assert_eq!(
        message_from(&target, output),
        GuiMessage::SelectSampleWithModifiers {
            path: String::from("sample.wav"),
            modifiers,
        }
    );
}

#[test]
/// Verifies sample-row double activation uses the normal activation action.
fn double_activation_uses_normal_sample_activation() {
    let bounds = ui::Rect::from_size(120.0, 22.0);
    let mut target = sample_hit_target(false, false, false, false);

    let output = dispatch(
        &mut target,
        bounds,
        WidgetInput::primary_double_click(Point::new(34.0, 8.0)),
    );
    assert_eq!(
        message_from(&target, output),
        GuiMessage::SelectSampleWithModifiers {
            path: String::from("sample.wav"),
            modifiers: PointerModifiers::default(),
        }
    );
}

#[test]
/// Verifies retained pressed and hover state survive a row refresh.
fn pressed_and_hover_state_survive_retained_widget_refresh() {
    let bounds = ui::Rect::from_size(120.0, 22.0);
    let mut previous = sample_hit_target(false, false, false, false);
    dispatch(
        &mut previous,
        bounds,
        WidgetInput::primary_press(Point::new(34.0, 8.0)),
    );
    assert!(sample_widget_is_hovered(&previous));
    assert!(sample_widget_is_pressed(&previous));

    let mut refreshed = sample_hit_target(false, false, false, false);
    synchronize_widget_from_previous(&mut refreshed, &previous);

    assert!(sample_widget_is_hovered(&refreshed));
    assert!(sample_widget_is_pressed(&refreshed));
}

#[test]
/// Verifies cached sample rows reserve a separate rail before the scrollbar.
fn loaded_rows_paint_readiness_marker_clear_of_scrollbar() {
    let bounds = ui::Rect::from_xy_size(10.0, 20.0, 120.0, 22.0);
    let target = sample_hit_target(false, false, false, true);
    let plan = sample_widget_plan(&target, bounds);

    assert!(
        plan.fill_rects().any(|fill| fill.rect.min.x
            == bounds.max.x - CACHED_MARKER_EDGE_INSET - 2.0
            && fill.rect.max.x
                == bounds.max.x - SAMPLE_LIST_SCROLLBAR_WIDTH - CACHED_MARKER_SCROLLBAR_GAP
            && fill.rect.width() == 2.0
            && fill.color == CACHED_MARKER),
        "loaded rows should show a near-white readiness marker clear of the scrollbar lane"
    );
}

#[test]
/// Verifies uncached sample rows do not paint the loaded marker.
fn unloaded_rows_do_not_paint_loaded_marker() {
    let bounds = ui::Rect::from_xy_size(10.0, 20.0, 120.0, 22.0);
    let target = sample_hit_target(false, false, false, false);
    let plan = sample_widget_plan(&target, bounds);

    assert!(
        !plan
            .fill_rects()
            .any(|fill| fill.rect.width() == 2.0 && fill.color == CACHED_MARKER),
        "unloaded rows should not show the loaded marker"
    );
}
