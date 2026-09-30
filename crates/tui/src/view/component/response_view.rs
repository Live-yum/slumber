//! Response views keep the network record immutable and plaintext ephemeral.
use crate::{
    message::Message,
    util::syntax::SyntaxType,
    view::{
        Component, ViewContext,
        common::header_table::HeaderTable,
        component::{
            Canvas, ComponentId, Draw, DrawMetadata, ToChild, internal::Child,
            queryable_body::QueryableBody,
        },
        context::UpdateContext,
        event::{BroadcastEvent, Emitter, Event, EventMatch},
        persistent::PersistentKey,
        util::view_text,
    },
};
use bytes::Bytes;
use futures::FutureExt;
use mime::Mime;
use ratatui::{
    layout::{Constraint, Layout},
    widgets::{Paragraph, Wrap},
};
use serde::{Serialize, Serializer};
use slumber_config::Action;
use slumber_core::{
    collection::{ProfileId, RecipeId},
    crypto::{CryptoError, transform_response},
    http::{ResponseBody, ResponseRecord},
};
use std::sync::Arc;

#[derive(Debug)]
pub struct ResponseBodyView {
    id: ComponentId,
    response: Arc<ResponseRecord>,
    recipe_id: RecipeId,
    profile_id: Option<ProfileId>,
    emitter: Emitter<TransformComplete>,
    generation: u64,
    has_transform: bool,
    show_raw: bool,
    transformed: Option<QueryableBody<ResponseQueryKey>>,
    transform_error: Option<CryptoError>,
    body: QueryableBody<ResponseQueryKey>,
}

#[derive(derive_more::Debug)]
struct TransformComplete {
    generation: u64,
    #[debug(skip)]
    result: Result<Bytes, CryptoError>,
}

impl ResponseBodyView {
    #[cfg(test)]
    pub fn new(recipe_id: RecipeId, response: Arc<ResponseRecord>) -> Self {
        Self::with_profile(recipe_id, None, response)
    }
    pub fn with_profile(
        recipe_id: RecipeId,
        profile_id: Option<ProfileId>,
        response: Arc<ResponseRecord>,
    ) -> Self {
        let config = ViewContext::config();
        let mime = response.mime();
        let default_query = config.default_query(mime.as_ref());
        let body = QueryableBody::new(
            ResponseQueryKey {
                recipe_id: recipe_id.clone(),
                mime,
                view: None,
            },
            Arc::clone(&response),
            default_query.map(String::from),
        );
        let has_transform = ViewContext::collection()
            .recipes
            .get_recipe(&recipe_id)
            .is_some_and(|recipe| !recipe.response_transform.is_empty());
        let mut view = Self {
            id: ComponentId::default(),
            response,
            recipe_id,
            profile_id,
            emitter: Emitter::default(),
            generation: 0,
            has_transform,
            show_raw: !has_transform,
            transformed: None,
            transform_error: None,
            body,
        };
        if has_transform {
            view.refresh_transform();
        }
        view
    }
    fn refresh_transform(&mut self) {
        self.generation += 1;
        self.transformed = None;
        self.transform_error = None;
        let generation = self.generation;
        let recipe_id = self.recipe_id.clone();
        let bytes = self.response.body.bytes().clone();
        let emitter = self.emitter;
        ViewContext::push_message(Message::TransformResponse {
            profile_id: self.profile_id.clone(),
            callback: Box::new(move |context| {
                async move {
                    let result =
                        transform_response(&context, &recipe_id, &bytes).await;
                    emitter.emit(TransformComplete { generation, result });
                }
                .boxed_local()
            }),
        });
    }
    fn visible_body(&self) -> Option<&QueryableBody<ResponseQueryKey>> {
        if self.show_raw {
            Some(&self.body)
        } else {
            self.transformed.as_ref()
        }
    }
    pub fn view_body(&self) {
        if let Some(body) = self.visible_body() {
            let mime = if self.show_raw {
                self.response.mime()
            } else {
                Some(mime::APPLICATION_JSON)
            };
            view_text(body.visible_text(), mime);
        }
    }
    pub fn copy_body(&self) {
        if let Some(body) = self.visible_body() {
            ViewContext::push_message(Message::CopyText(
                body.visible_text().to_string(),
            ));
        }
    }
    pub fn save_response_body(&self) {
        if let Some(body) = self.visible_body() {
            ViewContext::push_message(Message::SaveResponseBody {
                request_id: self.response.id,
                data: if self.show_raw {
                    body.modified_text()
                } else {
                    Some(body.visible_text().to_string())
                },
            });
        }
    }
}
impl Component for ResponseBodyView {
    fn id(&self) -> ComponentId {
        self.id
    }
    fn update(&mut self, _: &mut UpdateContext, event: Event) -> EventMatch {
        event
            .m()
            .broadcast(|event| {
                if self.has_transform
                    && event == BroadcastEvent::RefreshPreviews
                {
                    self.refresh_transform();
                }
            })
            .emitted(self.emitter, |complete| {
                if complete.generation != self.generation {
                    return;
                }
                match complete.result {
                    Ok(bytes) => {
                        self.transformed = Some(QueryableBody::from_body(
                            ResponseQueryKey {
                                recipe_id: self.recipe_id.clone(),
                                mime: Some(mime::APPLICATION_JSON),
                                view: Some("transformed"),
                            },
                            ResponseBody::new(bytes),
                            Some(SyntaxType::Json),
                            None,
                        ))
                    }
                    Err(error) => self.transform_error = Some(error),
                }
            })
            .action(|action, propagate| match action {
                Action::View => self.view_body(),
                Action::ToggleResponseView if self.has_transform => {
                    self.show_raw = !self.show_raw
                }
                _ => propagate.set(),
            })
    }
    fn children(&mut self) -> Vec<Child<'_>> {
        if self.show_raw {
            vec![self.body.to_child()]
        } else {
            self.transformed
                .as_mut()
                .map(|body| vec![body.to_child()])
                .unwrap_or_default()
        }
    }
}
impl Draw for ResponseBodyView {
    fn draw(&self, canvas: &mut Canvas, (): (), metadata: DrawMetadata) {
        let area = if self.has_transform {
            let [label, area] =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
                    .areas(metadata.area());
            let binding =
                ViewContext::binding_display(Action::ToggleResponseView);
            let view = if self.show_raw {
                "Raw network body"
            } else if self.transform_error.is_some() {
                "Transform FAILED (raw preserved)"
            } else if self.transformed.is_none() {
                "Transforming response..."
            } else {
                "Transformed JSON (not persisted)"
            };
            canvas.render_widget(
                Paragraph::new(format!("{view} | {binding} raw/transformed")),
                label,
            );
            area
        } else {
            metadata.area()
        };
        if let Some(body) = self.visible_body() {
            canvas.draw(body, (), area, true);
        } else if let Some(error) = &self.transform_error {
            canvas.render_widget(
                Paragraph::new(error.to_string()).wrap(Wrap { trim: false }),
                area,
            );
        }
    }
}

/// Persisted key for response body JSONPath query text box
#[derive(Debug, Serialize)]
struct ResponseQueryKey {
    recipe_id: RecipeId,
    #[serde(skip_serializing_if = "Option::is_none")]
    view: Option<&'static str>,
    /// Separate queries by MIME so an HTML error does not receive a JSON
    /// query.
    #[serde(serialize_with = "serialize_mime")]
    mime: Option<Mime>,
}
impl PersistentKey for ResponseQueryKey {
    type Value = String;
}

#[expect(clippy::ref_option)]
fn serialize_mime<S>(
    mime: &Option<Mime>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    mime.as_ref().map(Mime::as_ref).serialize(serializer)
}

#[derive(Debug)]
pub struct ResponseHeadersView {
    id: ComponentId,
    response: Arc<ResponseRecord>,
}
impl ResponseHeadersView {
    pub fn new(response: Arc<ResponseRecord>) -> Self {
        Self {
            id: ComponentId::default(),
            response,
        }
    }
}
impl Component for ResponseHeadersView {
    fn id(&self) -> ComponentId {
        self.id
    }
}
impl Draw for ResponseHeadersView {
    fn draw(&self, canvas: &mut Canvas, (): (), metadata: DrawMetadata) {
        canvas.render_widget(
            HeaderTable {
                headers: &self.response.headers,
            },
            metadata.area(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::test_util::{TestComponent, TestHarness, harness};
    use indexmap::indexmap;
    use rstest::rstest;
    use slumber_core::{http::Exchange, test_util::header_map};
    use slumber_util::{Factory, assert_matches};
    use terminput::KeyCode;

    #[rstest]
    #[case::text_body(
        ResponseRecord {
            body: br#"{"hello":"world"}"#.as_slice().into(),
            ..ResponseRecord::factory(())
        },
        "{\"hello\":\"world\"}",
    )]
    #[case::json_body(
        ResponseRecord {
            headers: header_map(indexmap! {"content-type" => "application/json"}),
            body: br#"{"hello":"world"}"#.as_slice().into(),
            ..ResponseRecord::factory(())
        },
        "{\n  \"hello\": \"world\"\n}",
    )]
    #[case::binary_body(
        ResponseRecord {
            body: b"\x01\x02\x03\xff".as_slice().into(),
            ..ResponseRecord::factory(())
        },
        "01 02 03 ff"
    )]
    #[tokio::test]
    async fn test_copy_body(
        mut harness: TestHarness,
        #[case] response: ResponseRecord,
        #[case] expected_body: &str,
    ) {
        let exchange = Exchange {
            response: response.into(),
            ..Exchange::factory(())
        };
        let component = TestComponent::new(
            &mut harness,
            ResponseBodyView::new(
                exchange.request.recipe_id.clone(),
                exchange.response,
            ),
        );
        component.copy_body();
        let body = assert_matches!(harness.messages_rx().try_pop(), Some(Message::CopyText(body)) => body);
        assert_eq!(body, expected_body);
    }

    #[rstest]
    #[case::text_body(
        ResponseRecord { body: b"hello!".as_slice().into(), ..ResponseRecord::factory(()) },
        None, None,
    )]
    #[case::json_body(
        ResponseRecord {
            headers: header_map(indexmap! {"content-type" => "application/json"}),
            body: br#"{"hello":"world"}"#.as_slice().into(),
            ..ResponseRecord::factory(())
        },
        None, Some("{\n  \"hello\": \"world\"\n}"),
    )]
    #[case::binary_body(
        ResponseRecord { body: b"\x01\x02\x03".as_slice().into(), ..ResponseRecord::factory(()) },
        None, None,
    )]
    #[case::queried_body(
        ResponseRecord { body: b"hello!".as_slice().into(), ..ResponseRecord::factory(()) },
        Some("head -c 4"), Some("hell"),
    )]
    #[tokio::test]
    async fn test_save_file(
        mut harness: TestHarness,
        #[case] response: ResponseRecord,
        #[case] query: Option<&str>,
        #[case] expected_body: Option<&str>,
    ) {
        let exchange_id = response.id;
        let exchange = Exchange {
            response: response.into(),
            ..Exchange::factory(exchange_id)
        };
        let mut component = TestComponent::new(
            &mut harness,
            ResponseBodyView::new(
                exchange.request.recipe_id.clone(),
                exchange.response,
            ),
        );
        if let Some(query) = query {
            component
                .int(&mut harness)
                .send_key(KeyCode::Char('/'))
                .send_text(query)
                .send_key(KeyCode::Enter)
                .run_task()
                .await
                .assert()
                .empty();
        }
        component.save_response_body();
        let (request_id, data) = assert_matches!(harness.messages_rx().try_pop(), Some(Message::SaveResponseBody { request_id, data }) => (request_id, data));
        assert_eq!(request_id, exchange.id);
        assert_eq!(data.as_deref(), expected_body);
    }
}
