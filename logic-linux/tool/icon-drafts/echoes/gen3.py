SQ=open('../tape/gen.py').read().split('SQ="')[1].split('"')[0]
W=5.2
def ring(cx,cy,r,w=W):
    return (f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="url(#glass)" stroke-width="{w}"/>'
            f'<circle cx="{cx}" cy="{cy}" r="{r+w/2-.55}" fill="none" stroke="url(#spec)" stroke-width=".9"/>'
            f'<circle cx="{cx}" cy="{cy}" r="{r-w/2+.5}" fill="none" stroke="url(#spec2)" stroke-width=".8"/>'
            f'<circle cx="{cx}" cy="{cy}" r="{r-w/2+.45}" fill="none" stroke="url(#ishade)" stroke-width=".8"/>')
cx=64; R=36; cy=64
B=cy+R
rr=[R,24.5,13]
base='<g opacity=".86">'+''.join(f'<circle cx="{cx}" cy="{B-r}" r="{r}" fill="none" stroke="url(#glassb)" stroke-width="{W}"/>' for r in rr)+'</g>'
parts=base.replace('<g opacity=".86">','<g opacity=".9" filter="url(#bevel)">')
svg=f'''<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128"><defs>
<clipPath id="sq"><path d="{SQ}"/></clipPath>
<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#5779aa"/><stop offset="1" stop-color="#2c466f"/></linearGradient>
<radialGradient id="glow" cx=".5" cy=".45" r=".5"><stop offset="0" stop-color="#a8c6f2" stop-opacity=".18"/><stop offset="1" stop-color="#a8c6f2" stop-opacity="0"/></radialGradient>
<linearGradient id="glass" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#d2e2fb" stop-opacity=".92"/><stop offset=".5" stop-color="#a9c3ea" stop-opacity=".78"/><stop offset="1" stop-color="#8eacda" stop-opacity=".85"/></linearGradient>
<linearGradient id="spec" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".95"/><stop offset=".45" stop-color="#fff" stop-opacity=".15"/><stop offset=".7" stop-color="#fff" stop-opacity="0"/><stop offset="1" stop-color="#fff" stop-opacity=".35"/></linearGradient>
<linearGradient id="spec2" x1="1" y1="1" x2="0" y2="0"><stop offset="0" stop-color="#fff" stop-opacity=".7"/><stop offset=".4" stop-color="#fff" stop-opacity="0"/></linearGradient>
<linearGradient id="ishade" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1c2f50" stop-opacity=".35"/><stop offset=".4" stop-color="#1c2f50" stop-opacity="0"/></linearGradient>
<linearGradient id="rim" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".55"/><stop offset=".35" stop-color="#fff" stop-opacity=".08"/><stop offset=".65" stop-color="#fff" stop-opacity=".02"/><stop offset="1" stop-color="#fff" stop-opacity=".25"/></linearGradient>
<linearGradient id="glassb" x1="0" y1="0" x2="0" y2="128" gradientUnits="userSpaceOnUse"><stop offset=".2" stop-color="#d4e3fb"/><stop offset=".55" stop-color="#abc4ea"/><stop offset=".85" stop-color="#90aedb"/></linearGradient><radialGradient id="dot" cx=".4" cy=".3" r=".75"><stop offset="0" stop-color="#e3eeff"/><stop offset="1" stop-color="#94b2de"/></radialGradient>
<linearGradient id="gloss" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".12"/><stop offset=".45" stop-color="#fff" stop-opacity=".02"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
<filter id="bevel" x="-10%" y="-10%" width="120%" height="120%">
<feGaussianBlur in="SourceAlpha" stdDeviation=".9" result="b"/>
<feSpecularLighting in="b" surfaceScale="2.2" specularConstant="1.1" specularExponent="22" lighting-color="#fff" result="s"><feDistantLight azimuth="225" elevation="42"/></feSpecularLighting>
<feComposite in="s" in2="SourceAlpha" operator="in" result="sp"/>
<feSpecularLighting in="b" surfaceScale="2.2" specularConstant=".55" specularExponent="18" lighting-color="#fff" result="s2"><feDistantLight azimuth="45" elevation="40"/></feSpecularLighting>
<feComposite in="s2" in2="SourceAlpha" operator="in" result="sp2"/>
<feComposite in="SourceGraphic" in2="sp" operator="arithmetic" k1="0" k2="1" k3=".85" k4="0" result="a"/>
<feComposite in="a" in2="sp2" operator="arithmetic" k1="0" k2="1" k3=".45" k4="0"/>
</filter>
<filter id="soft" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur in="SourceAlpha" stdDeviation="1.6"/><feOffset dy="1.5"/><feComponentTransfer><feFuncA type="linear" slope=".28"/></feComponentTransfer><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter>
</defs>
<g clip-path="url(#sq)">
<rect width="128" height="128" fill="url(#bg)"/><rect width="128" height="128" fill="url(#glow)"/>
<rect width="128" height="64" fill="url(#gloss)"/>
<g filter="url(#soft)">{parts}

</g>
<path d="{SQ}" fill="none" stroke="url(#rim)" stroke-width="2.2"/>
</g>
<path d="{SQ}" fill="none" stroke="#14233d" stroke-opacity=".35" stroke-width=".8"/></svg>'''
open('echoes-c.svg','w').write(svg)
