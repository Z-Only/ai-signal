import {build} from 'esbuild';
import {readdir,readFile,mkdir,writeFile,cp,rm} from 'node:fs/promises';
import {resolve,join} from 'node:path';
const root=resolve('..');
const output=join(root,'dist');
await rm(output,{recursive:true,force:true});
await mkdir(join(output,'server'),{recursive:true});
await mkdir('generated',{recursive:true});
const assets:Record<string,{body:string,type:string}>={};
async function scan(dir:string,prefix=''){for(const item of await readdir(dir,{withFileTypes:true})){const key=prefix+'/'+item.name;if(item.isDirectory())await scan(join(dir,item.name),key);else {const ext=item.name.split('.').pop();const type=ext==='js'?'application/javascript; charset=utf-8':ext==='css'?'text/css; charset=utf-8':ext==='svg'?'image/svg+xml':'text/html; charset=utf-8';assets[key]={body:await readFile(join(dir,item.name),'utf8'),type};}}}
await scan(join(root,'frontend/dist'));
await writeFile('generated/assets.json',JSON.stringify(assets));
await writeFile('generated/entry.ts',`import {initSync,public_sources_json,normalize_feed_json} from './ai_news_core.js';\nimport wasmModule from './ai_news_core_bg.wasm';\nimport assets from './assets.json';\nimport {createRustCore,createSite} from '../src/runtime';\ninitSync({module:wasmModule});\nexport default createSite(createRustCore({public_sources_json,normalize_feed_json}),assets);\n`);
await build({entryPoints:['generated/entry.ts'],outfile:join(output,'server/index.js'),bundle:true,format:'esm',target:'es2022',platform:'browser',external:['*.wasm'],minify:true});
await cp('generated/ai_news_core_bg.wasm',join(output,'server/ai_news_core_bg.wasm'));
await mkdir(join(output,'.openai'),{recursive:true});
let hosting={d1:'DB',r2:null};try{hosting=JSON.parse(await readFile(join(root,'.openai/hosting.json'),'utf8'));}catch{}
await writeFile(join(output,'.openai/hosting.json'),JSON.stringify(hosting,null,2));
await cp(join(root,'drizzle'),join(output,'.openai/drizzle'),{recursive:true});
console.log('Built Vue + Rust/WASM Worker to dist/server');
