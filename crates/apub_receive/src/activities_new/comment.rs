use crate::inbox::new_inbox_routing::{ReceiveActivity, Activity, PublicUrl};
use url::Url;
use lemmy_apub::NoteExt;
use activitystreams::
    activity::kind::{CreateType};
use lemmy_websocket::{LemmyContext, UserOperationCrud};
use lemmy_utils::LemmyError;
use lemmy_db_views::comment_view::CommentView;
use lemmy_api_common::comment::CommentResponse;
use lemmy_websocket::messages::SendComment;
use lemmy_api_common::{send_local_notifs, blocking};
use lemmy_db_schema::source::comment::Comment;
use lemmy_apub::objects::FromApub;
use lemmy_utils::utils::scrape_text_for_mentions;
use lemmy_db_schema::source::post::Post;
use lemmy_db_queries::Crud;
use lemmy_apub::fetcher::person::get_or_fetch_and_upsert_person;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateComment {
    actor: Url,
    to: PublicUrl,
    object: NoteExt,
    #[serde(rename = "type")]
    kind: CreateType,
}

#[async_trait::async_trait(?Send)]
impl ReceiveActivity for Activity<CreateComment> {
    async fn receive(&self, context: &LemmyContext, request_counter: &mut i32) -> Result<(), LemmyError> {
        let comment =
            Comment::from_apub(&self.inner.object, context, self.inner.actor.clone(), request_counter, false).await?;

        let post_id = comment.post_id;
        let post = blocking(context.pool(), move |conn| Post::read(conn, post_id)).await??;
        let actor = get_or_fetch_and_upsert_person(&self.inner.actor, context, request_counter).await?;

        // Note:
        // Although mentions could be gotten from the post tags (they are included there), or the ccs,
        // Its much easier to scrape them from the comment body, since the API has to do that
        // anyway.
        let mentions = scrape_text_for_mentions(&comment.content);
        let recipient_ids = send_local_notifs(
            mentions,
            comment.clone(),
            actor,
            post,
            context.pool(),
            true,
        )
            .await?;

        // Refetch the view
        let comment_view = blocking(context.pool(), move |conn| {
            CommentView::read(conn, comment.id, None)
        })
            .await??;

        let res = CommentResponse {
            comment_view,
            recipient_ids,
            form_id: None,
        };

        context.chat_server().do_send(SendComment {
            op: UserOperationCrud::CreateComment,
            comment: res,
            websocket_id: None,
        });

        Ok(())
    }
}