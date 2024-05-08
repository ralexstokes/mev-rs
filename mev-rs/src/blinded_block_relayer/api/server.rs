use crate::{
    blinded_block_provider::{
        api::server::{
            handle_fetch_bid, handle_open_bid, handle_status_check, handle_validator_registration,
        },
        BlindedBlockProvider,
    },
    blinded_block_relayer::{
        BlindedBlockDataProvider, BlindedBlockRelayer, BlockSubmissionFilter,
        DeliveredPayloadFilter, ValidatorRegistrationQuery,
    },
    error::Error,
    types::{
        block_submission::data_api::{PayloadTrace, SubmissionTrace},
        ProposerSchedule, SignedBidSubmission, SignedValidatorRegistration,
    },
};
use axum::{
    extract::{Json, Query, State},
    response::Html,
    routing::{get, post, IntoMakeService},
    Router,
};
use hyper::server::conn::AddrIncoming;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::task::JoinHandle;
use tracing::{error, info, trace};

/// Type alias for the configured axum server
pub type BlockRelayServer = axum::Server<AddrIncoming, IntoMakeService<Router>>;

// cribbed from: https://github.com/tbjgolden/simple-json-viewer
const ROOT_HTML_TRAILER: &str = r#"
<script>
(function(a){function b(d){if(c[d])return c[d].exports;var e=c[d]={i:d,l:!1,exports:{}};return a[d].call(e.exports,e,e.exports,b),e.l=!0,e.exports}var c={};return b.m=a,b.c=c,b.d=function(a,c,d){b.o(a,c)||Object.defineProperty(a,c,{configurable:!1,enumerable:!0,get:d})},b.n=function(a){var c=a&&a.__esModule?function(){return a['default']}:function(){return a};return b.d(c,'a',c),c},b.o=function(a,b){return Object.prototype.hasOwnProperty.call(a,b)},b.p='',b(b.s=0)})([function(){'use strict';function a(a){if(Array.isArray(a)){for(var b=0,c=Array(a.length);b<a.length;b++)c[b]=a[b];return c}return Array.from(a)}function b(a,b){if(!(a instanceof b))throw new TypeError('Cannot call a class as a function')}var c=function(){function a(a,b){var c,d=[],e=!0,f=!1;try{for(var g,h=a[Symbol.iterator]();!(e=(g=h.next()).done)&&(d.push(g.value),!(b&&d.length===b));e=!0);}catch(a){f=!0,c=a}finally{try{!e&&h['return']&&h['return']()}finally{if(f)throw c}}return d}return function(b,c){if(Array.isArray(b))return b;if(Symbol.iterator in Object(b))return a(b,c);throw new TypeError('Invalid attempt to destructure non-iterable instance')}}(),d=function(){function a(a,b){for(var c,d=0;d<b.length;d++)c=b[d],c.enumerable=c.enumerable||!1,c.configurable=!0,'value'in c&&(c.writable=!0),Object.defineProperty(a,c.key,c)}return function(b,c,d){return c&&a(b.prototype,c),d&&a(b,d),b}}(),e=Object.assign||function(a){for(var b,c=1;c<arguments.length;c++)for(var d in b=arguments[c],b)Object.prototype.hasOwnProperty.call(b,d)&&(a[d]=b[d]);return a},f='function'==typeof Symbol&&'symbol'==typeof Symbol.iterator?function(a){return typeof a}:function(a){return a&&'function'==typeof Symbol&&a.constructor===Symbol&&a!==Symbol.prototype?'symbol':typeof a};(function(){function g(b){var c=1<arguments.length&&void 0!==arguments[1]?arguments[1]:[];return[].concat(a(b.childNodes||[])).forEach(function(a){[1,3].includes(a.nodeType)&&(c.push(a),1===a.nodeType&&g(a,c))}),c}function h(a){do if(a=a.parentNode,!a||a.classList.contains('json-viewer'))return;while(!a.classList.contains('json-viewer-collection'));return a}function i(a,b){if(a instanceof RegExp)return a.test(b);if(!a||!b)return!1;a=a.toLowerCase(),b=b.toLowerCase();for(var c=0,d=0;;){if(c===a.length)return!0;if(d===b.length)return!1;a.charCodeAt(c)===b.charCodeAt(d)&&c++,d++}}function j(a,b){var c=document.createElement('STYLE');c.innerHTML=''.replace(/\[timestamp\]/g,'.'+a).replace(/([0-9])vmax/g,function(a,c){return b.colors[~~c]}).replace('monospace',b.fontFamily),document.head.appendChild(c)}function k(a){return a=JSON.stringify(a),a.substring(1,a.length-1)}function l(a){var b='undefined'==typeof a?'undefined':f(a);return'object'===b?Array.isArray(a)?'array':'object':b}function m(a){return'object'===('undefined'==typeof HTMLElement?'undefined':f(HTMLElement))?a instanceof HTMLElement:a&&'object'===('undefined'==typeof a?'undefined':f(a))&&null!==a&&1===a.nodeType&&'string'==typeof a.nodeName}var n={colors:['#999','#090','#c00','#c0c','#00c','#ccc','#333','#ff0','#eee'],fontFamily:'monospace'};document.addEventListener('DOMContentLoaded',function(){var a=document.createElement('STYLE');a.innerHTML='.json-viewer-container{display:flex;flex-direction:column}.json-viewer-container *{white-space:pre;font-family:monospace;font-size:14px;line-height:16px;letter-spacing:0;box-sizing:border-box}.json-viewer-search{display:block;border:1px solid;border-bottom-width:0;padding:2.5ex;z-index:2}.json-viewer{display:block;border:1px solid;padding:2.5ex;user-select:none;overflow:auto;z-index:1}.json-viewer-object:before{content:"{"}.json-viewer-object:after{content:"}"}.json-viewer-array:before{content:"["}.json-viewer-array:after{content:"]"}.json-viewer-string:before,.json-viewer-string:after{content:\'"\'}.json-viewer-object-value:before{content:": "}.json-viewer-array-value:not(:last-child):after,.json-viewer-key-value-pair:not(:last-child):after{content:",\\A"}.json-viewer-array-contents,.json-viewer-object-contents{display:block;padding-left:5ex}.json-viewer-toggle{border:0;padding:0 .4ex;margin:0;outline:0;background:none}.json-viewer-object .json-viewer-toggle:before,.json-viewer-array .json-viewer-toggle:before{content:"><";font-size:1.8ex}.json-viewer-object .json-viewer-toggle:hover:before,.json-viewer-array .json-viewer-toggle:hover:before{cursor:pointer}.json-viewer-object.json-viewer-closed .json-viewer-toggle:before,.json-viewer-array.json-viewer-closed .json-viewer-toggle:before{content:"<>"}.json-viewer-object.json-viewer-closed .json-viewer-object-contents,.json-viewer-array.json-viewer-closed .json-viewer-array-contents{display:none}.json-viewer-array-contents{counter-reset:index -1}.json-viewer-array-value{counter-increment:index;position:relative}.json-viewer-searching .json-viewer-array-value:before,.json-viewer-array-value:hover:before{content:counter(index);position:absolute;display:inline-block;top:.1ex;left:0;transform:translate3d(calc(-100% - .5ex), 0, 0)}.json-viewer :before,.json-viewer :after{font:inherit;line-height:inherit}',document.head.appendChild(a)}),window.createJSONViewer=function(a){var b=1<arguments.length&&void 0!==arguments[1]?arguments[1]:{},c=2<arguments.length&&void 0!==arguments[2]?arguments[2]:{};if(!m(a))throw Error('createJSONViewer must be called with a HTML element');else if(!['string','object'].includes('undefined'==typeof b?'undefined':f(b)))throw Error('json parameter passed to createJSONViewer is not a string or object');else if('object'!==('undefined'==typeof c?'undefined':f(c)))throw Error('options parameter passed to createJSONViewer is not an object');return'string'!=typeof b&&(b=JSON.stringify(b)),new o(a,b,e({},n,c))};var o=function(){function e(a,c,d){b(this,e);var f=document.createElement('DIV');f.classList.add('json-viewer');var g=document.createElement('INPUT');g.setAttribute('placeholder','Search'),g.classList.add('json-viewer-search'),g.addEventListener('keyup',this.searchInputKeyUp.bind(this)),a.classList.add('json-viewer-container'),this.container=a,this.el=f,this.searchEl=g,this.searchValue='',this.json=c,this.opts=d,this.values=[],this.container.appendChild(g),this.container.appendChild(f),this.uid='time-'+Date.now(),a.classList.add(this.uid),j(this.uid,this.opts),this.generate()}return d(e,[{key:'generate',value:function(){var a=this;this.values=[];var b=this.htmlify(JSON.parse(this.json))||'';this.el.innerHTML=b,this.el.querySelectorAll('[data-json-id]').forEach(function(b){b._value=a.values[b.getAttribute('data-json-id')]}),this.attachListeners(),this.allNodes=g(this.el)}},{key:'getJSON',value:function(){return JSON.parse(this.json)}},{key:'changeJSON',value:function(){var a=0<arguments.length&&void 0!==arguments[0]?arguments[0]:{},b=1<arguments.length&&void 0!==arguments[1]?arguments[1]:[];'string'==typeof a&&(a=JSON.parse(a));for(var c=this.getJSON(),d=c;1<b.length;)d=d[b.shift()];b.length?(d[b[0]]=a,this.json=JSON.stringify(c)):this.json=JSON.stringify(a),this.generate(),this.searchValue=null,this.searchInputKeyUp()}},{key:'searchInputKeyUp',value:function(){var a=this;clearTimeout(this.debounce),this.debounce=setTimeout(function(){var b=a.searchEl.value,c=a.searchValue;b!==c&&(b?(a.el.classList.add('json-viewer-searching'),a.search(b)):(a.el.classList.remove('json-viewer-searching'),a.allNodes.forEach(function(a){return 1===a.nodeType?a.classList.contains('json-viewer-collection')&&a.classList.remove('json-viewer-closed'):void a.parentNode.classList.remove('json-viewer-match')}))),a.searchValue=b},200)}},{key:'search',value:function(a){var b=null;try{if(!/\/[^/]*\/[\s\S]*/.test(a))throw new Error;var d=function(a){return[a[0]?null:a.slice(1,a.length-1).join('/'),a[a.length-1]]}(a.split('/')),e=c(d,2),f=e[0],g=e[1];b=new RegExp(f,g)}catch(a){b=null}this.searchEl.style.color=b?this.opts.colors[4]:this.opts.colors[6],this.allNodes.forEach(function(c){if(1===c.nodeType)return c.classList.contains('json-viewer-collection')&&c.classList.add('json-viewer-closed');if(i(b||a,c.textContent)){c.parentNode.classList.add('json-viewer-match');for(var d=h(c);d;)d.classList.remove('json-viewer-closed'),d=h(d)}else c.parentNode.classList.remove('json-viewer-match')})}},{key:'htmlify',value:function(b){var c=this,d=1<arguments.length&&void 0!==arguments[1]?arguments[1]:[],e=l(b);return'number'===e||'boolean'===e?'<span class="json-viewer-'+e+'">'+b+'</span>':'string'===e?'<span class="json-viewer-string">'+k(b)+'</span>':'array'===e?'<span class="json-viewer-array json-viewer-collection"><button class="json-viewer-toggle"></button><span class="json-viewer-array-contents">'+b.map(function(b,e){return c.values.push(b),'<span class="json-viewer-array-value" data-index="'+e+'" data-json-id="'+(c.values.length-1)+'" data-location="'+encodeURIComponent(JSON.stringify(d))+'">'+c.htmlify(b,[].concat(a(d),[e]))+'</span>'}).join('')+'</span></span>':'object'===e?'<span class="json-viewer-object json-viewer-collection"><button class="json-viewer-toggle"></button><span class="json-viewer-object-contents">'+Object.keys(b).map(function(e){return c.values.push(e),c.values.push(b[e]),'<span class="json-viewer-key-value-pair" data-location="'+encodeURIComponent(JSON.stringify(d))+'"><span class="json-viewer-object-key" data-json-id="'+(c.values.length-2)+'">'+k(e)+'</span><span class="json-viewer-object-value" data-json-id="'+(c.values.length-1)+'">'+c.htmlify(b[e],[].concat(a(d),[e]))+'</span></span>'}).join('')+'</span></span>':void 0}},{key:'attachListeners',value:function(){var b=this;this.el.querySelectorAll('.json-viewer-toggle').forEach(function(a){a.addEventListener('click',function(a){a.target.parentNode.classList.toggle('json-viewer-closed')})}),this.el.querySelectorAll('.json-viewer-key-value-pair').forEach(function(c){c.addEventListener('click',function(c){if(c.stopPropagation(),b.opts.onValueClick){for(var d=c.target;!d.classList.contains('json-viewer-key-value-pair');)d=d.parentNode;b.opts.onValueClick(d.querySelector('.json-viewer-object-value')._value,[].concat(a(JSON.parse(decodeURIComponent(d.getAttribute('data-location')))),[d.querySelector('.json-viewer-object-key')._value]),d)}})}),this.el.querySelectorAll('.json-viewer-array-value').forEach(function(c){c.addEventListener('click',function(c){if(c.stopPropagation(),b.opts.onValueClick){for(var d=c.target;!d.classList.contains('json-viewer-array-value');)d=d.parentNode;b.opts.onValueClick(d._value,[].concat(a(JSON.parse(decodeURIComponent(d.getAttribute('data-location')))),[~~d.getAttribute('data-index')]),d)}})})}}]),e}()})()}]);
</script>

<div id="json-viewer-container"></div>
</section>
</main>
</body>

<script>
  var container = document.querySelector('#json-viewer-container');

  loadData();

  // refresh every 12 seconds
  setInterval(loadData, 12000);

  function loadData() {
    const proposer_payloads = fetch("/relay/v1/data/bidtraces/proposer_payload_delivered").then(response => response.json());

    const builder_submissions = fetch("/relay/v1/data/bidtraces/builder_blocks_received").then(response => response.json());

    Promise.all([proposer_payloads, builder_submissions]).then(([proposer_payloads, builder_submissions]) => {
      const data = {
          "proposer_payloads_delivered" : proposer_payloads,
          "builder_blocks_received": builder_submissions,
      };
      var options = {
        fontFamily: '"Fira Mono", monospace',
        colors: ['gray', '#090', '#c00', 'purple', '#00c', '#ccc', '#333', 'yellow', 'rgb(240,240,240)']
      };

      container.innerHTML = "";
      var viewer = createJSONViewer(container, data, options);
    })
  }
</script>
</html>
"#;

async fn handle_get_root<R: BlindedBlockDataProvider>(
    State(relay): State<R>,
) -> Result<Html<String>, Error> {
    trace!("serving root");
    let mut response = format!(
        r#"
        <html>
        <head>
        <link
          rel="stylesheet"
          href="https://cdn.jsdelivr.net/npm/@picocss/pico@2/css/pico.min.css"
        />
        </head>
        <body>
        <header>
          <h1> mev-relay-rs </h1>
        </header>
        <main>
          <section>
          <p>relay public key: {0:?} </p>
          <p>registered validators: {1}</p>
          </section>
          <section>
    "#,
        relay.public_key(),
        relay.registered_validators_count(),
    );
    response.push_str(ROOT_HTML_TRAILER);
    Ok(Html(response))
}

async fn handle_get_proposal_schedule<R: BlindedBlockRelayer>(
    State(relay): State<R>,
) -> Result<Json<Vec<ProposerSchedule>>, Error> {
    trace!("serving proposal schedule for current and next epoch");
    Ok(Json(relay.get_proposal_schedule().await?))
}

async fn handle_submit_bid<R: BlindedBlockRelayer>(
    State(relay): State<R>,
    Json(signed_bid_submission): Json<SignedBidSubmission>,
) -> Result<(), Error> {
    trace!("handling bid submission");
    relay.submit_bid(&signed_bid_submission).await
}

async fn handle_get_proposer_payloads_delivered<R: BlindedBlockDataProvider>(
    State(relay): State<R>,
    Query(filters): Query<DeliveredPayloadFilter>,
) -> Result<Json<Vec<PayloadTrace>>, Error> {
    trace!("handling proposer payloads delivered");
    Ok(Json(relay.get_delivered_payloads(&filters).await?))
}

async fn handle_get_builder_blocks_received<R: BlindedBlockDataProvider>(
    State(relay): State<R>,
    Query(filters): Query<BlockSubmissionFilter>,
) -> Result<Json<Vec<SubmissionTrace>>, Error> {
    trace!("handling block submissions");
    Ok(Json(relay.get_block_submissions(&filters).await?))
}

async fn handle_get_validator_registration<R: BlindedBlockDataProvider>(
    State(relay): State<R>,
    Query(params): Query<ValidatorRegistrationQuery>,
) -> Result<Json<SignedValidatorRegistration>, Error> {
    trace!("handling fetch validator registration");
    Ok(Json(relay.fetch_validator_registration(&params.public_key).await?))
}

pub struct Server<R> {
    host: Ipv4Addr,
    port: u16,
    relay: R,
}

impl<
        R: BlindedBlockRelayer
            + BlindedBlockProvider
            + BlindedBlockDataProvider
            + Clone
            + Send
            + Sync
            + 'static,
    > Server<R>
{
    pub fn new(host: Ipv4Addr, port: u16, relay: R) -> Self {
        Self { host, port, relay }
    }

    /// Configures and returns the axum server
    pub fn serve(&self) -> BlockRelayServer {
        let router = Router::new()
            .route("/", get(handle_get_root::<R>))
            .route("/eth/v1/builder/status", get(handle_status_check))
            .route("/eth/v1/builder/validators", post(handle_validator_registration::<R>))
            .route(
                "/eth/v1/builder/header/:slot/:parent_hash/:public_key",
                get(handle_fetch_bid::<R>),
            )
            .route("/eth/v1/builder/blinded_blocks", post(handle_open_bid::<R>))
            .route("/relay/v1/builder/validators", get(handle_get_proposal_schedule::<R>))
            .route("/relay/v1/builder/blocks", post(handle_submit_bid::<R>))
            .route(
                "/relay/v1/data/bidtraces/proposer_payload_delivered",
                get(handle_get_proposer_payloads_delivered::<R>),
            )
            .route(
                "/relay/v1/data/bidtraces/builder_blocks_received",
                get(handle_get_builder_blocks_received::<R>),
            )
            .route(
                "/relay/v1/data/validator_registration",
                get(handle_get_validator_registration::<R>),
            )
            .with_state(self.relay.clone());
        let addr = SocketAddr::from((self.host, self.port));
        axum::Server::bind(&addr).serve(router.into_make_service())
    }

    /// Spawns the server on a new task returning the handle for it
    pub fn spawn(&self) -> JoinHandle<()> {
        let server = self.serve();
        let addr = server.local_addr();
        tokio::spawn(async move {
            info!("listening at {addr}...");
            if let Err(err) = server.await {
                error!(%err, "error while listening for incoming")
            }
        })
    }
}
