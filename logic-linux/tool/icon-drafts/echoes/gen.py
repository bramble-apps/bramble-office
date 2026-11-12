SQ=open('../tape/gen.py').read().split('SQ="')[1].split('"')[0]
tx,ty=64,85; sr=7.5; sy=ty+10
rings=[(31,2.6,1.0),(21.5,2.1,.78),(13,1.8,.6)]
rs=''.join(f'<circle cx="{tx}" cy="{ty-r}" r="{r}" fill="none" stroke="url(#ring)" stroke-width="{w}" stroke-opacity="{o}"/>' for r,w,o in rings)
big=rings[0][0]; cyb=ty-big
svg=f'''<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128"><defs>
<clipPath id="sq"><path d="{SQ}"/></clipPath>
<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#5a7cab"/><stop offset="1" stop-color="#2b4369"/></linearGradient>
<radialGradient id="glow" cx=".5" cy=".4" r=".55"><stop offset="0" stop-color="#9cc0f0" stop-opacity=".2"/><stop offset="1" stop-color="#9cc0f0" stop-opacity="0"/></radialGradient>
<linearGradient id="ring" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#dbe8ff"/><stop offset="1" stop-color="#9ab9e6"/></linearGradient>
<linearGradient id="cone" x1="0" y1="1" x2="0" y2="0"><stop offset="0" stop-color="#cfe0fb" stop-opacity=".28"/><stop offset="1" stop-color="#cfe0fb" stop-opacity=".03"/></linearGradient>
<radialGradient id="disc" cx=".45" cy=".35" r=".7"><stop offset="0" stop-color="#a9c4ec"/><stop offset="1" stop-color="#6f8fbf"/></radialGradient>
<linearGradient id="gloss" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".16"/><stop offset=".45" stop-color="#fff" stop-opacity=".02"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
<filter id="soft" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur in="SourceAlpha" stdDeviation="1.4"/><feOffset dy="1.2"/><feComponentTransfer><feFuncA type="linear" slope=".3"/></feComponentTransfer><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter>
</defs>
<g clip-path="url(#sq)">
<rect width="128" height="128" fill="url(#bg)"/><rect width="128" height="128" fill="url(#glow)"/>
<g filter="url(#soft)">
<path d="M{tx-5} {sy} L{tx-big} {cyb} A{big} {big} 0 0 1 {tx+big} {cyb} L{tx+5} {sy} Z" fill="url(#cone)"/>
{rs}
<circle cx="{tx}" cy="{sy}" r="{sr}" fill="url(#disc)"/>
<circle cx="{tx}" cy="{sy}" r="{sr}" fill="none" stroke="#dbe8ff" stroke-opacity=".7" stroke-width="1.1"/>
<circle cx="{tx}" cy="{sy}" r="2.1" fill="#2b4369"/>
</g>
<rect width="128" height="64" fill="url(#gloss)"/>
</g></svg>'''
open('echoes-a.svg','w').write(svg)
