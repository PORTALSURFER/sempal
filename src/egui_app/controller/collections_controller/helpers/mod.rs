mod members;
mod ui;
mod r#move;

use super::*;
use std::path::PathBuf;

pub(crate) struct CollectionsController<'a> {
    controller: &'a mut EguiController,
}

pub(super) struct CollectionMemberView<'a> {
    source_id: &'a SourceId,
    relative_path: &'a Path,
    clip_root: Option<&'a PathBuf>,
}

impl<'a> CollectionMemberView<'a> {
    fn from_member(member: &'a CollectionMember) -> Self {
        Self {
            source_id: &member.source_id,
            relative_path: &member.relative_path,
            clip_root: member.clip_root.as_ref(),
        }
    }
}

#[derive(Clone)]
pub(super) struct CollectionMemberViewOwned {
    source_id: SourceId,
    relative_path: PathBuf,
    clip_root: Option<PathBuf>,
}

impl CollectionMemberViewOwned {
    fn from_member(member: &CollectionMember) -> Self {
        Self {
            source_id: member.source_id.clone(),
            relative_path: member.relative_path.clone(),
            clip_root: member.clip_root.clone(),
        }
    }

    fn as_view(&self) -> CollectionMemberView<'_> {
        CollectionMemberView {
            source_id: &self.source_id,
            relative_path: &self.relative_path,
            clip_root: self.clip_root.as_ref(),
        }
    }
}

impl<'a> CollectionsController<'a> {
    pub(crate) fn new(controller: &'a mut EguiController) -> Self {
        Self { controller }
    }
}

impl std::ops::Deref for CollectionsController<'_> {
    type Target = EguiController;

    fn deref(&self) -> &Self::Target {
        self.controller
    }
}

impl std::ops::DerefMut for CollectionsController<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.controller
    }
}

fn collection_member_view(member: &CollectionMember) -> CollectionMemberView<'_> {
    CollectionMemberView::from_member(member)
}

fn collection_member_view_owned(member: &CollectionMember) -> CollectionMemberViewOwned {
    CollectionMemberViewOwned::from_member(member)
}
