#!/usr/bin/env python3
"""Build a reviewed, offline WaveSpeed registry from saved public model pages.
No model code or price formula is executed. The output is a release asset, never
live discovery or a job-supplied schema. Re-run tests and review before shipping.
"""
import argparse
import copy
import hashlib
import json
import re
from pathlib import Path
from wavespeed_price_parser import Parser

REF_ROLES = {'image':'image','images':'image','reference_images':'image','last_image':'last',
             'mask_image':'mask','mask_url':'mask','markup_image':'markup','keyframes':'keyframe',
             'reference_videos':'video','videos':'video','reference_audios':'audio','audios':'audio'}

def clean(schema):
    if isinstance(schema,list): return [clean(x) for x in schema]
    if not isinstance(schema,dict): return schema
    if isinstance(schema.get('pattern'),str) and re.fullmatch(r'\$[0-9a-fA-F]+',schema['pattern']):raise ValueError('Resolve the public-page text reference before import')
    return {k:clean(v) for k,v in schema.items() if not k.startswith('x-') and k not in {'description','title','examples'}}

def example_ref(role):
    kind='video' if role=='video' else 'audio' if role=='audio' else 'image'
    return {'url':'https://api.everyframe.studio/v1/attachment/att_example?t='+'a'*32,
            'sha256':'a'*64,'bytes':123,'mime':{'image':'image/png','audio':'audio/mpeg','video':'video/mp4'}[kind],
            'role':role,'width':1024 if kind!='audio' else 0,'height':1024 if kind!='audio' else 0,
            'durationMs':5000 if kind!='image' else None,'frameCount':120 if kind=='video' else None}

def model_entry(ident,model):
    if not re.fullmatch(r'[a-zA-Z0-9_-]+(?:/[a-zA-Z0-9_.-]+){1,3}',ident): raise ValueError('Unsafe model ID')
    raw=next(s for s in model['api_schema']['api_schemas'] if s['type']=='model_run')
    assert raw['method']=='POST' and raw['server']=='https://api.wavespeed.ai'
    assert raw['api_path']=='/api/v3/'+ident
    schema=clean(raw['request_schema']);props=schema['properties'];required=schema.get('required',[])
    kind='video' if 'video' in model['type'] else 'audio' if any(t in model['type'] for t in ['audio','speech','music']) else 'image'
    prompt='text' if 'text' in props and 'prompt' not in props else 'prompt'
    assert prompt in props
    params={k:copy.deepcopy(p['default']) for k,p in props.items() if 'default' in p and k!=prompt}
    refs=[];paths=[];roles={}
    for key,base_role in REF_ROLES.items():
        if key not in props:continue
        prop=props[key];role='first' if key=='image' and kind=='video' else base_role
        is_array=prop.get('type')=='array'
        paths.append('/'+key+('/*' if is_array else ''));roles[paths[-1]]=role
        if key in required:
            index=len(refs);refs.append(example_ref(role))
            params[key]=['@reference:'+str(index)] if is_array else '@reference:'+str(index)
            if is_array:prop['minItems']=max(1,prop.get('minItems',0))
    if 'voice_id' in required:params['voice_id']='Wise_Woman'
    if 'format' in props:params.setdefault('format','mp3')
    if 'sample_rate' in props:params.setdefault('sample_rate',44100)
    if 'bitrate' in props:params.setdefault('bitrate',128000)
    max_prompt=props[prompt].get('maxLength',10000 if prompt=='text' else 1000000)
    props[prompt]['maxLength']=max_prompt
    for key,role in roles.items():
        field=key.split('/')[1];p=props[field]
        if p.get('type')=='array':p.setdefault('maxItems',16)
    if 'seed' in props:
        props['seed'].setdefault('minimum',-1);props['seed'].setdefault('maximum',2147483647)
    name=ident.rsplit('/',1)[-1].replace('-',' ').title()
    family=ident.split('/')[1].replace('-',' ').title()
    title=family if len(ident.split('/'))==2 else family+' · '+name
    return {'provider':'wavespeed','apiModel':ident,'name':title+' (WaveSpeedAI)',
        'vendor':ident.split('/')[0], 'family':family,'kind':kind,'nativeMedia':True,
        'adapter':'schema-wavespeed','nativeSchema':schema,'apiPath':raw['api_path'],
        'apiPrompt':'/'+prompt,'apiModelField':None,'params':params,'exampleReferences':refs,
        'referencePaths':paths,'referenceRoles':roles,'referenceTransport':'owned-url',
        'pricingRule':{'type':'wavespeed','baseMicrousd':model['base_price'],'formula':model.get('formula',''),
            'ast':Parser(model['formula']).parse() if model.get('formula') else ['get','base_price']},
        'mime':'video/mp4' if kind=='video' else 'audio/mpeg' if kind=='audio' else 'image/png',
        'duration':params.get('duration',5 if kind=='video' else None),'resolution':params.get('resolution','1k' if kind=='image' else 'audio'),
        'aspect':params.get('aspect_ratio','source' if refs else '1:1'),'dimensions':[],
        'codecs':['h264','hevc'] if kind=='video' else ['mjpeg','png','webp'],
        'audio':kind=='video','seed':False,'maxPrompt':max_prompt,'deadlineMs':1200000,
        'feeMultiplier':1,'schemaUrl':'https://wavespeed.ai/models/'+ident,'schemaCheckedAt':'2026-11-10',
        'validationStatus':'documentation-reviewed','nativeCapabilities':{'fields':list(props),'maxReferences':sum((props[path.split('/')[1]].get('maxItems',16) if path.endswith('/*') else 1) for path in paths),
            'resolutions':props.get('resolution',{}).get('enum',[]),'aspects':props.get('aspect_ratio',{}).get('enum',[]),
            'qualities':props.get('quality',{}).get('enum',[]),'formats':props.get('output_format',props.get('format',{})).get('enum',[])},
        'sourceSha256':hashlib.sha256(json.dumps(model,sort_keys=True,separators=(',',':')).encode()).hexdigest()}

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--catalog',required=True);parser.add_argument('--output',required=True);args=parser.parse_args()
    catalog=json.loads(Path(args.catalog).read_text());out={}
    for ident,model in catalog.items():out['studio/wavespeed/'+ident]=model_entry(ident,model)
    Path(args.output).write_text(json.dumps(out,indent=2,ensure_ascii=False)+'\n')
    print('Reviewed WaveSpeed endpoints:',len(out))
if __name__=='__main__':main()
