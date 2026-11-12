SQ=open('../tape/gen.py').read().split('SQ="')[1].split('"')[0]
cx=64; B=106
L={'big':(41,'#2f9a9c','#0e4d57'),'mid':(28,'#4cbab3','#17707a'),'small':(15,'#8fe6d8','#36a59f')}
def build(order,name):
    defs=''; body=''; prev=None
    for i,k in enumerate(order):
        r,c0,c1=L[k]; cy=B-r
        defs+=f'<linearGradient id="g{k}" gradientUnits="userSpaceOnUse" x1="0" y1="{cy-r}" x2="0" y2="{cy+r}"><stop offset="0" stop-color="{c0}"/><stop offset="1" stop-color="{c1}"/></linearGradient>'
        defs+=f'<clipPath id="c{k}"><circle cx="{cx}" cy="{cy}" r="{r}"/></clipPath>'
        layer=''
        # shadow cast on what is below
        layer+=f'<circle cx="{cx}" cy="{cy+2.2}" r="{r}" fill="#000" opacity=".38" filter="url(#sh)"/>'
        if prev:  # frosted view of the layers behind
            layer+=f'<g clip-path="url(#c{k})"><use href="#{prev}" filter="url(#frost)"/></g>'
        layer+=f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="url(#g{k})" opacity="{.55 if prev else .96}"/>'
        layer+=f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="url(#sheen)" opacity=".5"/>'
        layer+=f'<circle cx="{cx}" cy="{cy}" r="{r-.6}" fill="none" stroke="url(#rim)" stroke-width="1.1"/>'
        gid=f'L{i}'
        body=f'<g id="{gid}">{body}{layer}</g>' if body else f'<g id="{gid}">{layer}</g>'
        prev=gid
    svg=f'''<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="128" height="128" viewBox="0 0 128 128"><defs>
<clipPath id="sq"><path d="{SQ}"/></clipPath>
<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#14525b"/><stop offset=".55" stop-color="#0b313a"/><stop offset="1" stop-color="#05161c"/></linearGradient>
<linearGradient id="rim" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".85"/><stop offset=".35" stop-color="#fff" stop-opacity=".18"/><stop offset=".75" stop-color="#fff" stop-opacity=".05"/><stop offset="1" stop-color="#cff" stop-opacity=".3"/></linearGradient>
<linearGradient id="sheen" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".35"/><stop offset=".45" stop-color="#fff" stop-opacity="0"/></linearGradient>
<linearGradient id="tilerim" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".35"/><stop offset=".25" stop-color="#fff" stop-opacity=".05"/><stop offset="1" stop-color="#fff" stop-opacity=".1"/></linearGradient>
<filter id="sh" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="2.4"/></filter>
<filter id="frost" x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur stdDeviation=".7"/><feColorMatrix type="saturate" values=".85"/></filter>
{defs}</defs>
<g clip-path="url(#sq)"><rect width="128" height="128" fill="url(#bg)"/>{body}
<path d="{SQ}" fill="none" stroke="url(#tilerim)" stroke-width="2"/></g></svg>'''
    open(name,'w').write(svg)
build(['big','mid','small'],'stack-large-to-small.svg')
build(['small','mid','big'],'stack-small-to-large.svg')
