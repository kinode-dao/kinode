import{B as e,g as a,s as t,d as s,i as r,c as n,a as o,e as c,H as l,b as d}from"./index-DzA96B0X.js";class u extends e{constructor({callbackSelector:e,cause:t,data:s,extraData:r,sender:n,urls:o}){var c;super(t.shortMessage||"An error occurred while fetching for an offchain result.",{cause:t,metaMessages:[...t.metaMessages||[],(null==(c=t.metaMessages)?void 0:c.length)?"":[],"Offchain Gateway Call:",o&&["  Gateway URL(s):",...o.map((e=>`    ${a(e)}`))],`  Sender: ${n}`,`  Data: ${s}`,`  Callback selector: ${e}`,`  Extra data: ${r}`].flat()}),Object.defineProperty(this,"name",{enumerable:!0,configurable:!0,writable:!0,value:"OffchainLookupError"})}}class i extends e{constructor({result:e,url:s}){super("Offchain gateway response is malformed. Response data must be a hex value.",{metaMessages:[`Gateway URL: ${a(s)}`,`Response: ${t(e)}`]}),Object.defineProperty(this,"name",{enumerable:!0,configurable:!0,writable:!0,value:"OffchainLookupResponseMalformedError"})}}class f extends e{constructor({sender:e,to:a}){super("Reverted sender address does not match target contract address (`to`).",{metaMessages:[`Contract address: ${a}`,`OffchainLookup sender address: ${e}`]}),Object.defineProperty(this,"name",{enumerable:!0,configurable:!0,writable:!0,value:"OffchainLookupSenderMismatchError"})}}const p="0x556f1830",b={name:"OffchainLookup",type:"error",inputs:[{name:"sender",type:"address"},{name:"urls",type:"string[]"},{name:"callData",type:"bytes"},{name:"callbackFunction",type:"bytes4"},{name:"extraData",type:"bytes"}]};async function h(e,{blockNumber:a,blockTag:t,data:l,to:d}){const{args:i}=s({data:l,abi:[b]}),[p,h,y,w,g]=i,{ccipRead:k}=e,x=k&&"function"==typeof(null==k?void 0:k.request)?k.request:m;try{if(!r(d,p))throw new f({sender:p,to:d});const s=await x({data:y,sender:p,urls:h}),{data:l}=await n(e,{blockNumber:a,blockTag:t,data:o([w,c([{type:"bytes"},{type:"bytes"}],[s,g])]),to:d});return l}catch(O){throw new u({callbackSelector:w,cause:O,data:l,extraData:g,sender:p,urls:h})}}async function m({data:e,sender:a,urls:s}){var r;let n=new Error("An unknown error occurred.");for(let c=0;c<s.length;c++){const u=s[c],f=u.includes("{data}")?"GET":"POST",p="POST"===f?{data:e,sender:a}:void 0;try{const s=await fetch(u.replace("{sender}",a).replace("{data}",e),{body:JSON.stringify(p),method:f});let o;if(o=(null==(r=s.headers.get("Content-Type"))?void 0:r.startsWith("application/json"))?(await s.json()).data:await s.text(),!s.ok){n=new l({body:p,details:(null==o?void 0:o.error)?t(o.error):s.statusText,headers:s.headers,status:s.status,url:u});continue}if(!d(o)){n=new i({result:o,url:u});continue}return o}catch(o){n=new l({body:p,details:o.message,url:u})}}throw n}export{m as ccipRequest,h as offchainLookup,b as offchainLookupAbiItem,p as offchainLookupSignature};