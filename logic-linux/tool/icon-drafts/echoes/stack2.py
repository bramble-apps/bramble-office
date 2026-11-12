exec(open('stack.py').read().split("def build")[0])
W=112*.067
def build(layers,name,ring=False,blur=3.5,front=.55):
    defs=''; body=''; prev=None
    for i,(r,c0,c1) in enumerate(layers):
        cy=B-r; k=f'k{i}'
        defs+=f'<linearGradient id="g{k}" gradientUnits="userSpaceOnUse" x1="0" y1="{cy-r}" x2="0" y2="{cy+r}"><stop offset="0" stop-color="{c0}"/><stop offset="1" stop-color="{c1}"/></linearGradient>'
        if ring:
            shape=lambda fill,extra='':f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="{fill}" stroke-width="{W:.2f}" {extra}/>'
            defs+=f'<mask id="m{k}">{shape("#fff")}</mask>'
            rims=(f'<circle cx="{cx}" cy="{cy}" r="{r+W/2-.55:.2f}" fill="none" stroke="url(#rim)" stroke-width="1"/>'
                  f'<circle cx="{cx}" cy="{cy}" r="{r-W/2+.55:.2f}" fill="none" stroke="url(#rim2)" stroke-width=".8"/>')
        else:
            shape=lambda fill,extra='':f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{fill}" {extra}/>'
            defs+=f'<mask id="m{k}">{shape("#fff")}</mask>'
            rims=f'<circle cx="{cx}" cy="{cy}" r="{r-.6}" fill="none" stroke="url(#rim)" stroke-width="1.1"/>'
        layer=f'<g transform="translate(0 2.2)" opacity=".4" filter="url(#sh)">{shape("#000")}</g>'
        if prev: layer+=f'<g mask="url(#m{k})">'+('<rect width="128" height="128" fill="url(#bg)"/>' if ring else '')+f'<use href="#{prev}" filter="url(#frost)"/></g>'
        layer+=shape(f'url(#g{k})',f'opacity="{front if prev else .96}"')
        layer+=f'<g mask="url(#m{k})"><circle cx="{cx}" cy="{cy}" r="{r+W}" fill="url(#sheen)" opacity=".5"/></g>'+rims
        gid=f'L{i}'; body=f'<g id="{gid}">{body}{layer}</g>'; prev=gid
    svg=open('stack-large-to-small.svg').read()
    head=svg.split('<linearGradient id="gbig"')[0]
    head=head.replace('<filter id="sh"','<linearGradient id="rim2" x1="0" y1="1" x2="0" y2="0"><stop offset="0" stop-color="#fff" stop-opacity=".55"/><stop offset=".4" stop-color="#fff" stop-opacity="0"/></linearGradient><filter id="sh"')
    if ring: head=head.replace('<feGaussianBlur stdDeviation=".7"/>',f'<feGaussianBlur stdDeviation="{blur}"/>')
    out=f'''{head}{defs}</defs>
<g clip-path="url(#sq)"><rect width="128" height="128" fill="url(#bg)"/>{body}
<path d="{SQ}" fill="none" stroke="url(#tilerim)" stroke-width="2"/></g></svg>'''
    open(name,'w').write(out)
four=[(41,'#2a9295','#0d4a54'),(31,'#3eaaa7','#14636c'),(21,'#5fc6bd','#1f7f84'),(11,'#99eadb','#3aa8a1')]
build(four,'stack4-large-back.svg')
R=112*.40-W/2
rings=[(R,'#a6dcd3','#5aa9a2'),(R*.64,'#b8e6de','#6ab7ae'),(R*.30,'#cdf0e9','#7cc6bc')]
build(rings,'rings-large-back.svg',ring=True)
build(rings[::-1],'rings-large-front.svg',ring=True)

build(rings[::-1],'rings-front-frost-a.svg',ring=True,blur=3.5,front=.6)
build(rings[::-1],'rings-front-frost-b.svg',ring=True,blur=6,front=.68)
