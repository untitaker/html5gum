//! See [`examples/scraper.rs`] for usage.
use std::convert::Infallible;

use crate::emitters::callback::{Callback, CallbackEmitter, CallbackEvent};
use crate::utils::trace_log;
use crate::{Emitter, Error, Readable, Reader, State, Tokenizer};

use html5ever::interface::{create_element, TreeSink};
use html5ever::tokenizer::states::State as Html5everState;
use html5ever::tokenizer::{
    states::RawKind, Doctype, Tag, TagKind, Token as Html5everToken, TokenSink, TokenSinkResult,
};
use html5ever::tree_builder::TreeBuilder;
use html5ever::ParseOpts;
use html5ever::{Attribute, QualName};

const BOGUS_LINENO: u64 = 1;

#[derive(Debug)]
struct OurCallback<'a, S> {
    sink: &'a mut S,
    current_start_tag: Option<Tag>,
    next_state: Option<State>,
}

impl<'a, S: TokenSink> OurCallback<'a, S> {
    fn handle_sink_result<H>(&mut self, result: TokenSinkResult<H>) {
        match result {
            TokenSinkResult::Continue => {}
            TokenSinkResult::Script(_) => {
                self.next_state = Some(State::Data);
                // TODO: suspend tokenizer for script
            }
            TokenSinkResult::Plaintext => {
                self.next_state = Some(State::PlainText);
            }
            TokenSinkResult::RawData(RawKind::Rcdata) => {
                self.next_state = Some(State::RcData);
            }
            TokenSinkResult::RawData(RawKind::Rawtext) => {
                self.next_state = Some(State::RawText);
            }
            TokenSinkResult::RawData(RawKind::ScriptData) => {
                self.next_state = Some(State::ScriptData);
            }
            TokenSinkResult::RawData(RawKind::ScriptDataEscaped(_)) => {
                todo!()
            }
        }
    }

    fn sink_token(&mut self, token: Html5everToken) {
        trace_log!("sink_token: {:?}", token);
        let result = self.sink.process_token(token, BOGUS_LINENO);
        self.handle_sink_result(result);
    }
}

impl<'a, S: TokenSink> Callback<Infallible> for OurCallback<'a, S> {
    fn handle_event(&mut self, event: CallbackEvent<'_>) -> Option<Infallible> {
        trace_log!("Html5everEmitter::handle_event: {:?}", event);
        match event {
            CallbackEvent::OpenStartTag { name } => {
                self.current_start_tag = Some(Tag {
                    kind: TagKind::StartTag,
                    name: String::from_utf8_lossy(name).into_owned().into(),
                    self_closing: false,
                    attrs: Default::default(),
                });
            }
            CallbackEvent::AttributeName { name } => {
                if let Some(ref mut tag) = self.current_start_tag {
                    tag.attrs.push(Attribute {
                        name: QualName::new(
                            None,
                            Default::default(),
                            String::from_utf8_lossy(name).into_owned().into(),
                        ),
                        value: Default::default(),
                    });
                }
            }
            CallbackEvent::AttributeValue { value } => {
                if let Some(ref mut tag) = self.current_start_tag {
                    if let Some(attr) = tag.attrs.last_mut() {
                        attr.value.push_slice(&String::from_utf8_lossy(value));
                    }
                }
            }
            CallbackEvent::CloseStartTag { self_closing } => {
                if let Some(mut tag) = self.current_start_tag.take() {
                    tag.self_closing = self_closing;
                    self.sink_token(Html5everToken::TagToken(tag));
                }
            }
            CallbackEvent::EndTag { name } => {
                self.sink_token(Html5everToken::TagToken(Tag {
                    kind: TagKind::EndTag,
                    name: String::from_utf8_lossy(name).into_owned().into(),
                    self_closing: false,
                    attrs: Default::default(),
                }));
            }
            CallbackEvent::String { value } => {
                let mut first = true;
                for part in String::from_utf8_lossy(value).split('\0') {
                    if !first {
                        self.sink_token(Html5everToken::NullCharacterToken);
                    }

                    first = false;
                    self.sink_token(Html5everToken::CharacterTokens(part.to_owned().into()));
                }
            }
            CallbackEvent::Comment { value } => {
                self.sink_token(Html5everToken::CommentToken(
                    String::from_utf8_lossy(value).into_owned().into(),
                ));
            }
            CallbackEvent::Doctype {
                name,
                public_identifier,
                system_identifier,
                force_quirks,
            } => {
                self.sink_token(Html5everToken::DoctypeToken(Doctype {
                    name: Some(name)
                        .filter(|x| !x.is_empty())
                        .map(|x| String::from_utf8_lossy(x).into_owned().into()),
                    public_id: public_identifier
                        .map(|x| String::from_utf8_lossy(x).into_owned().into()),
                    system_id: system_identifier
                        .map(|x| String::from_utf8_lossy(x).into_owned().into()),
                    force_quirks,
                }));
            }
            CallbackEvent::Error(error) => {
                self.sink_token(Html5everToken::ParseError(error.as_str().into()));
            }
        }

        None
    }
}

/// A compatibility layer that allows you to plug the TreeBuilder from html5ever into the tokenizer
/// from html5gum.
///
/// See [`examples/scraper.rs`] for usage.
#[derive(Debug)]
pub struct Html5everEmitter<'a, S: TokenSink> {
    emitter_inner: CallbackEmitter<OurCallback<'a, S>>,
}

impl<'a, S: TokenSink> Html5everEmitter<'a, S> {
    /// Construct the compatibility layer.
    pub fn new(sink: &'a mut S) -> Self {
        Html5everEmitter {
            emitter_inner: CallbackEmitter::new(OurCallback {
                sink,
                current_start_tag: None,
                next_state: None,
            }),
        }
    }
}

impl<'a, S: TokenSink> Emitter for Html5everEmitter<'a, S> {
    type Token = Infallible;

    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>) {
        self.emitter_inner.set_last_start_tag(last_start_tag)
    }

    fn emit_eof(&mut self) {
        self.emitter_inner.emit_eof();
        let sink = &mut self.emitter_inner.callback_mut().sink;
        let _ignored = sink.process_token(Html5everToken::EOFToken, BOGUS_LINENO);
        sink.end();
    }

    fn emit_error(&mut self, error: Error) {
        self.emitter_inner.emit_error(error)
    }

    fn should_emit_errors(&mut self) -> bool {
        self.emitter_inner.should_emit_errors()
    }

    fn pop_token(&mut self) -> Option<Self::Token> {
        self.emitter_inner.pop_token()
    }
    fn emit_string(&mut self, c: &[u8]) {
        self.emitter_inner.emit_string(c)
    }

    fn init_start_tag(&mut self) {
        self.emitter_inner.init_start_tag()
    }

    fn init_end_tag(&mut self) {
        self.emitter_inner.init_end_tag()
    }

    fn init_comment(&mut self) {
        self.emitter_inner.init_comment()
    }

    fn emit_current_tag(&mut self) -> Option<State> {
        assert!(self.emitter_inner.emit_current_tag().is_none());
        self.emitter_inner.callback_mut().next_state.take()
    }

    fn emit_current_comment(&mut self) {
        self.emitter_inner.emit_current_comment()
    }

    fn emit_current_doctype(&mut self) {
        self.emitter_inner.emit_current_doctype()
    }

    fn set_self_closing(&mut self) {
        self.emitter_inner.set_self_closing()
    }

    fn set_force_quirks(&mut self) {
        self.emitter_inner.set_force_quirks()
    }

    fn push_tag_name(&mut self, s: &[u8]) {
        self.emitter_inner.push_tag_name(s)
    }

    fn push_comment(&mut self, s: &[u8]) {
        self.emitter_inner.push_comment(s)
    }

    fn push_doctype_name(&mut self, s: &[u8]) {
        self.emitter_inner.push_doctype_name(s)
    }

    fn init_doctype(&mut self) {
        self.emitter_inner.init_doctype()
    }

    fn init_attribute(&mut self) {
        self.emitter_inner.init_attribute()
    }

    fn push_attribute_name(&mut self, s: &[u8]) {
        self.emitter_inner.push_attribute_name(s)
    }

    fn push_attribute_value(&mut self, s: &[u8]) {
        self.emitter_inner.push_attribute_value(s)
    }

    fn set_doctype_public_identifier(&mut self, value: &[u8]) {
        self.emitter_inner.set_doctype_public_identifier(value)
    }

    fn set_doctype_system_identifier(&mut self, value: &[u8]) {
        self.emitter_inner.set_doctype_system_identifier(value)
    }

    fn push_doctype_public_identifier(&mut self, s: &[u8]) {
        self.emitter_inner.push_doctype_public_identifier(s)
    }

    fn push_doctype_system_identifier(&mut self, s: &[u8]) {
        self.emitter_inner.push_doctype_system_identifier(s)
    }

    fn current_is_appropriate_end_tag_token(&mut self) -> bool {
        self.emitter_inner.current_is_appropriate_end_tag_token()
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&mut self) -> bool {
        self.emitter_inner
            .callback_mut()
            .sink
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

fn map_tokenizer_state(input: Html5everState) -> State {
    match input {
        Html5everState::Data => State::Data,
        Html5everState::Plaintext => State::PlainText,
        Html5everState::RawData(RawKind::Rcdata) => State::RcData,
        Html5everState::RawData(RawKind::Rawtext) => State::RawText,
        Html5everState::RawData(RawKind::ScriptData) => State::ScriptData,
        x => todo!("{:?}", x),
    }
}

/// Parse an HTML fragment
///
/// This is a convenience function for using [Html5everEmitter] together with html5ever. It is
/// equivalent to the same functions in [html5ever::driver].
///
/// ```
/// use html5ever::{local_name, QualName, ns, namespace_url}; // extern crate html5ever;
/// use scraper::Html; // extern crate scraper;
///
/// let input = "<h1>hello world</h1>";
///
/// // equivalent to `Html::parse_fragment`
/// let dom = Html::new_fragment();
/// let Ok(dom) = html5gum::emitters::html5ever::parse_fragment(
///     input,
///     dom,
///     Default::default(),
///     QualName::new(None, ns!(html), local_name!("body")),
///     Vec::new()
/// );
/// ```
pub fn parse_fragment<'a, R, Sink>(
    input: R,
    mut sink: Sink,
    opts: ParseOpts,
    context_name: QualName,
    context_attrs: Vec<Attribute>,
) -> Result<Sink, <R::Reader as Reader>::Error>
where
    R: Readable<'a>,
    Sink: TreeSink,
{
    let context_elem = create_element(&mut sink, context_name, context_attrs);
    parse_fragment_for_element(input, sink, opts, context_elem, None)
}

/// Like `parse_fragment`, but with an existing context element
/// and optionally a form element.
///
/// This is a convenience function for using [Html5everEmitter] together with html5ever. It is
/// equivalent to the same functions in [html5ever::driver].
pub fn parse_fragment_for_element<'a, R, Sink>(
    input: R,
    sink: Sink,
    opts: ParseOpts,
    context_element: Sink::Handle,
    form_element: Option<Sink::Handle>,
) -> Result<Sink, <R::Reader as Reader>::Error>
where
    R: Readable<'a>,
    Sink: TreeSink,
{
    let mut tree_builder =
        TreeBuilder::new_for_fragment(sink, context_element, form_element, opts.tree_builder);

    let initial_state = map_tokenizer_state(tree_builder.tokenizer_state_for_context_elem());
    let token_emitter = Html5everEmitter::new(&mut tree_builder);
    let mut tokenizer = Tokenizer::new_with_emitter(input, token_emitter);
    tokenizer.set_state(initial_state);
    tokenizer.finish()?;
    Ok(tree_builder.sink)
}

/// Parse an HTML document.
///
/// This is a convenience function for using [Html5everEmitter] together with html5ever. It is
/// equivalent to the same functions in [html5ever::driver].
///
/// ```rust
/// use scraper::Html; // extern crate scraper;
///
/// let input = "<h1>hello world</h1>";
///
/// // equivalent to `Html::parse_document`
/// let dom = Html::new_document();
/// let Ok(dom) = html5gum::emitters::html5ever::parse_document(
///     input,
///     dom,
///     Default::default()
/// );
/// ```
pub fn parse_document<'a, R, Sink>(
    input: R,
    sink: Sink,
    opts: ParseOpts,
) -> Result<Sink, <R::Reader as Reader>::Error>
where
    R: Readable<'a>,
    Sink: TreeSink,
{
    let mut tree_builder = TreeBuilder::new(sink, opts.tree_builder);
    let token_emitter = Html5everEmitter::new(&mut tree_builder);
    let tokenizer = Tokenizer::new_with_emitter(input, token_emitter);
    tokenizer.finish()?;
    Ok(tree_builder.sink)
}
