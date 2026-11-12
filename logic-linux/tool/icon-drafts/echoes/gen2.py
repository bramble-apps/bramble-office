SQ=open('../tape/gen.py').read().split('SQ="')[1].split('"')[0]
W=5.2
def ring(cx,cy,r,w=W):
    return (f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="url(#glass)" stroke-width="{w}"/>'
            f'<circle cx="{cx}" cy="{cy}" r="{r+w/2-.55}" fill="none" stroke="url(#spec)" stroke-width=".9"/>'
            f'<circle cx="{cx}" cy="{cy}" r="{r-w/2+.5}" fill="none" stroke="url(#spec2)" stroke-width=".8"/>'
            f'<circle cx="{cx}" cy="{cy}" r="{r-w/2+.45}" fill="none" stroke="url(#ishade)" stroke-width=".8"/>')
cx=64; R=36; cy=64
parts=ring(cx,cy,R)
b=cy+R; r2=23.5; parts+=ring(cx,b-9-r2,r2,W*.85)
b2=b-9; r3=12; parts+=ring(cx,b2-8-r3,r3,W*.7)
dy=b2-8-r3+4.5
svg=f'''<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128"><defs>
<clipPath id="sq"><path d="{SQ}"/></clipPath>
<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#5779aa"/><stop offset="1" stop-color="#2c466f"/></linearGradient>
<radialGradient id="glow" cx=".5" cy=".45" r=".5"><stop offset="0" stop-color="#a8c6f2" stop-opacity=".18"/><stop offset="1" stop-color="#a8c6f2" stop-opacity="0"/></radialGradient>
<linearGradient id="glass" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#d2e2fb" stop-opacity=".92"/><stop offset=".5" stop-color="#a9c3ea" stop-opacity=".78"/><stop offset="1" stop-color="#8eacda" stop-opacity=".85"/></linearGradient>
<linearGradient id="spec" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".95"/><stop offset=".45" stop-color="#fff" stop-opacity=".15"/><stop offset=".7" stop-color="#fff" stop-opacity="0"/><stop offset="1" stop-color="#fff" stop-opacity=".35"/></linearGradient>
<linearGradient id="spec2" x1="1" y1="1" x2="0" y2="0"><stop offset="0" stop-color="#fff" stop-opacity=".7"/><stop offset=".4" stop-color="#fff" stop-opacity="0"/></linearGradient>
<linearGradient id="ishade" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1c2f50" stop-opacity=".35"/><stop offset=".4" stop-color="#1c2f50" stop-opacity="0"/></linearGradient>
<linearGradient id="rim" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".55"/><stop offset=".35" stop-color="#fff" stop-opacity=".08"/><stop offset=".65" stop-color="#fff" stop-opacity=".02"/><stop offset="1" stop-color="#fff" stop-opacity=".25"/></linearGradient>
<radialGradient id="dot" cx=".4" cy=".3" r=".75"><stop offset="0" stop-color="#e3eeff"/><stop offset="1" stop-color="#94b2de"/></radialGradient>
<linearGradient id="gloss" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".12"/><stop offset=".45" stop-color="#fff" stop-opacity=".02"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
<filter id="soft" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur in="SourceAlpha" stdDeviation="1.6"/><feOffset dy="1.5"/><feComponentTransfer><feFuncA type="linear" slope=".28"/></feComponentTransfer><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter>
</defs>
<g clip-path="url(#sq)">
<rect width="128" height="128" fill="url(#bg)"/><rect width="128" height="128" fill="url(#glow)"/>
<rect width="128" height="64" fill="url(#gloss)"/>
<g filter="url(#soft)">{parts}
<circle cx="{cx}" cy="{dy}" r="3.6" fill="url(#dot)"/><circle cx="{cx}" cy="{dy}" r="3.6" fill="none" stroke="#fff" stroke-opacity=".5" stroke-width=".6"/>
</g>
<path d="{SQ}" fill="none" stroke="url(#rim)" stroke-width="2.2"/>
</g>
<path d="{SQ}" fill="none" stroke="#14233d" stroke-opacity=".35" stroke-width=".8"/></svg>'''
open('echoes-b.svg','w').write(svg)
