"""Compile the small arithmetic subset in reviewed WaveSpeed public price formulas.
This only parses source text. The runtime interpreter has no eval, file, network,
reflection or user-defined function access.
"""
import json,re
class Parser:
    token=re.compile(r'\s*(?:("(?:[^"\\]|\\.)*")|(\d+(?:\.\d+)?)|([A-Za-z_$][A-Za-z0-9_$]*)|(:=|!=|<=|>=|[+*/<>=?:;,()\[\]-]))')
    precedence={':=':1,'or':3,'and':4,'=':5,'!=':5,'<':6,'>':6,'<=':6,'>=':6,'+':7,'-':7,'*':8,'/':8}
    def __init__(self,source):
        m=re.fullmatch(r'\s*\{\s*"total_price"\s*:\s*(.*)\}\s*',source,re.S)
        if not m:raise ValueError('Expected total_price formula')
        s=m.group(1);self.tokens=[];at=0
        while at<len(s.rstrip()):
            m=self.token.match(s,at)
            if not m:raise ValueError('Unknown token '+s[at:at+30])
            self.tokens.append(next(x for x in m.groups() if x is not None));at=m.end()
        self.i=0
    def peek(self):return self.tokens[self.i] if self.i<len(self.tokens) else None
    def pop(self,expected=None):
        v=self.peek()
        if v is None or expected is not None and v!=expected:raise ValueError(f'Expected {expected}, got {v}')
        self.i+=1;return v
    def sequence(self,close):
        parts=[self.expr()]
        while self.peek()==';':self.pop();parts.append(self.expr())
        self.pop(close)
        return parts[0] if len(parts)==1 else ['sequence',parts]
    def expr(self,minimum=0):
        token=self.pop()
        if token=='(':left=self.sequence(')')
        elif token=='[':
            values=[]
            if self.peek()!=']':
                while True:
                    values.append(self.expr())
                    if self.peek()!=',':break
                    self.pop()
            self.pop(']');left=['array',values]
        elif token=='-':left=['negate',self.expr(9)]
        elif token.startswith('"'):left=['literal',json.loads(token)]
        elif token[0].isdigit():left=['literal',float(token) if '.'in token else int(token)]
        elif token in ['true','false','null']:left=['literal',{'true':True,'false':False,'null':None}[token]]
        elif self.peek()=='(':
            self.pop();args=[];named={}
            if self.peek()!=')':
                while True:
                    if token=='get_duration' and self.i+1<len(self.tokens) and self.tokens[self.i+1]=='=':
                        key=self.pop();self.pop('=');named[key]=self.expr(6)
                    else:args.append(self.expr())
                    if self.peek()!=',':break
                    self.pop()
            self.pop(')')
            if token not in ['$min','$max','$ceil','$number','$lowercase','$string','$length','$count','$exists','get_duration']:raise ValueError('Unknown function '+token)
            left=['call',token,args,named]
        else:left=['get',token]
        while True:
            token=self.peek()
            if token=='?' and minimum<=2:
                self.pop();yes=self.expr();self.pop(':');no=self.expr(2);left=['if',left,yes,no];continue
            prec=self.precedence.get(token,-1)
            if prec<minimum:break
            self.pop();right=self.expr(prec if token==':=' else prec+1)
            left=['let',left[1],right] if token==':=' and left[0]=='get' else ['binary',token,left,right]
        return left
    def parse(self):
        out=self.expr()
        if self.i!=len(self.tokens):raise ValueError('Trailing tokens '+str(self.tokens[self.i:]))
        return out
if __name__=='__main__':
    import argparse
    from pathlib import Path
    parser=argparse.ArgumentParser();parser.add_argument('catalog');args=parser.parse_args()
    catalog=json.loads(Path(args.catalog).read_text())
    formulas=[m['formula'] for m in catalog.values() if m.get('formula')]
    for formula in formulas:Parser(formula).parse()
    print('Parsed reviewed price formulas:',len(formulas))
