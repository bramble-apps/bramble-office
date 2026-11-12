import math,random
random.seed(7)
SQ=open('gen.py').read().split('SQ="')[1].split('"')[0]
cx=cy=64; R=47
def pol(r,a): a=math.radians(a-90); return cx+r*math.cos(a), cy+r*math.sin(a)
def window(a0):
    r1,r2=R*.44,R*.83; h1,h2=11,21
    p=[pol(r1,a0-h1),pol(r2,a0-h2),pol(r2,a0+h2),pol(r1,a0+h1)]
    return (f'M{p[0][0]:.2f} {p[0][1]:.2f} L{p[1][0]:.2f} {p[1][1]:.2f} A{r2} {r2} 0 0 1 {p[2][0]:.2f} {p[2][1]:.2f} '
            f'L{p[3][0]:.2f} {p[3][1]:.2f} A{r1} {r1} 0 0 0 {p[0][0]:.2f} {p[0][1]:.2f} Z')
wins=''.join(f'<path d="{window(a)}"/>' for a in (25,145,265))
bg=''.join(f'<path d="M0 {y:.2f} H128" stroke="{random.choice(["#fff","#7d8188"])}" stroke-opacity="{random.uniform(.08,.35):.2f}" stroke-width="{random.uniform(.2,.6):.2f}"/>' for y in [i*0.7 for i in range(185)])
spun=''.join(f'<circle cx="{cx}" cy="{cy}" r="{r:.2f}" fill="none" stroke="{random.choice(["#fff","#8a8f97"])}" stroke-opacity="{random.uniform(.05,.25):.2f}" stroke-width=".35"/>' for r in [R*.2+i*.55 for i in range(int(R*.8/.55))])
tapelines=''.join(f'<circle cx="{cx}" cy="{cy}" r="{r:.2f}" fill="none" stroke="{random.choice(["#1c0e06","#a0663c","#2a160a"])}" stroke-opacity="{random.uniform(.25,.7):.2f}" stroke-width="{random.uniform(.25,.6):.2f}"/>' for r in [R*.25+i*.6 for i in range(int(R*.6/.6))])
knurl=''.join(f'<rect x="{cx-1}" y="{cy-R*.3}" width="2.2" height="3.6" rx=".5" fill="#2c2d31" transform="rotate({k*22.5} {cx} {cy})"/>' for k in range(16))
screws=''.join((lambda x,y:f'<g><circle cx="{x:.2f}" cy="{y:.2f}" r="1.7" fill="url(#screw)"/><path d="M{x-1.1:.2f} {y:.2f} H{x+1.1:.2f} M{x:.2f} {y-1.1:.2f} V{y+1.1:.2f}" stroke="#555" stroke-width=".45"/></g>')(*pol(R*.36,a)) for a in (85,205,325))
svg=f'''<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128"><defs>
<clipPath id="sq"><path d="{SQ}"/></clipPath>
<linearGradient id="plate" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#eceef1"/><stop offset=".55" stop-color="#c9ccd1"/><stop offset="1" stop-color="#a4a8ae"/></linearGradient>
<radialGradient id="flange" cx=".38" cy=".28" r=".85"><stop offset="0" stop-color="#e9ebee"/><stop offset=".55" stop-color="#c9ccd1"/><stop offset="1" stop-color="#a3a7ae"/></radialGradient>
<filter id="matte" x="0" y="0" width="100%" height="100%"><feTurbulence type="fractalNoise" baseFrequency="1.6" numOctaves="2" seed="4"/><feColorMatrix values="0 0 0 0 .5  0 0 0 0 .5  0 0 0 0 .5  0 0 0 .22 0"/><feComposite in2="SourceGraphic" operator="in"/></filter>
<linearGradient id="sheen" x1="0" y1="1" x2="1" y2="0"><stop offset="0" stop-color="#fff" stop-opacity="0"/><stop offset=".42" stop-color="#fff" stop-opacity="0"/><stop offset=".5" stop-color="#fff" stop-opacity=".55"/><stop offset=".58" stop-color="#fff" stop-opacity="0"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
<radialGradient id="tape" cx=".5" cy=".5" r=".5"><stop offset=".3" stop-color="#2e1a0d"/><stop offset=".6" stop-color="#5a341c"/><stop offset=".85" stop-color="#6e4224"/><stop offset="1" stop-color="#3a2010"/></radialGradient>
<radialGradient id="tapeglow" cx=".38" cy=".3" r=".7"><stop offset="0" stop-color="#ffcf9a" stop-opacity=".5"/><stop offset=".5" stop-color="#ffb070" stop-opacity=".12"/><stop offset="1" stop-color="#000" stop-opacity=".25"/></radialGradient>
<radialGradient id="hub" cx=".4" cy=".35" r=".7"><stop offset="0" stop-color="#4a4b50"/><stop offset="1" stop-color="#0d0d0f"/></radialGradient>
<radialGradient id="screw" cx=".4" cy=".35" r=".7"><stop offset="0" stop-color="#fff"/><stop offset="1" stop-color="#8d929a"/></radialGradient>
<linearGradient id="gloss" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".25"/><stop offset=".4" stop-color="#fff" stop-opacity=".03"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
<mask id="win"><g fill="#fff" stroke="#fff" stroke-width="5" stroke-linejoin="round">{wins}</g></mask>
<mask id="flm"><circle cx="{cx}" cy="{cy}" r="{R}" fill="#fff"/><g fill="#000" stroke="#000" stroke-width="5" stroke-linejoin="round">{wins}</g></mask>
<filter id="drop" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur in="SourceAlpha" stdDeviation="2.4"/><feOffset dy="2.4"/><feComponentTransfer><feFuncA type="linear" slope=".5"/></feComponentTransfer><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter>
<mask id="shadowm"><g fill="#fff" stroke="#fff" stroke-width="5" stroke-linejoin="round">{wins}</g><g fill="#000" stroke="#000" stroke-width="5" stroke-linejoin="round" transform="translate(0 1.8)">{wins}</g></mask>
<mask id="wallm"><g fill="#fff" stroke="#fff" stroke-width="5" stroke-linejoin="round">{wins}</g><g fill="#000" stroke="#000" stroke-width="5" stroke-linejoin="round" transform="translate(0 -0.9)">{wins}</g></mask>
<filter id="inset" x="-5%" y="-5%" width="110%" height="110%"><feGaussianBlur stdDeviation=".6"/></filter>
</defs>
<g clip-path="url(#sq)">
<rect width="128" height="128" fill="url(#plate)"/>{bg}
<g filter="url(#drop)">
 <circle cx="{cx}" cy="{cy}" r="{R}" fill="#6b6f76"/>
 <g mask="url(#win)">
  <circle cx="{cx}" cy="{cy}" r="{R}" fill="#1a1b1e"/>
  <circle cx="{cx}" cy="{cy}" r="{R*.86}" fill="url(#tape)"/>{tapelines}
  <circle cx="{cx}" cy="{cy}" r="{R*.86}" fill="url(#tapeglow)"/>
  <circle cx="{cx}" cy="{cy}" r="{R*.86}" fill="url(#sheen)"/>
  <circle cx="{cx}" cy="{cy}" r="{R*.86}" fill="none" stroke="#c88a58" stroke-opacity=".6" stroke-width=".6"/>
  
 </g>
 <g mask="url(#shadowm)"><rect width="128" height="128" fill="#000" opacity=".42" filter="url(#inset)"/></g>
 <g mask="url(#wallm)"><rect width="128" height="128" fill="#d9dce0"/></g>
 <g mask="url(#flm)">
  <circle cx="{cx}" cy="{cy}" r="{R}" fill="url(#flange)"/><circle cx="{cx}" cy="{cy}" r="{R}" fill="#888" filter="url(#matte)"/>
  <circle cx="{cx}" cy="{cy}" r="{R-.6}" fill="none" stroke="#fff" stroke-opacity=".45" stroke-width=".8"/>
  <circle cx="{cx}" cy="{cy}" r="{R-2.2}" fill="none" stroke="#7d8188" stroke-opacity=".5" stroke-width=".5"/>
 </g>
 
 {screws}
 <circle cx="{cx}" cy="{cy}" r="{R*.3}" fill="url(#hub)"/>{knurl}
 <circle cx="{cx}" cy="{cy}" r="{R*.21}" fill="#141417" stroke="#3c3d42" stroke-width=".8"/>
 <circle cx="{cx}" cy="{cy}" r="{R*.07}" fill="#2a2b2f"/>
 {''.join(f'<rect x="{cx-1}" y="{cy-R*.15}" width="2" height="{R*.06:.2f}" fill="#000" transform="rotate({a} {cx} {cy})"/>' for a in (0,120,240))}
 <path d="M{cx-R*.2:.2f} {cy-R*.12:.2f} A{R*.24:.2f} {R*.24:.2f} 0 0 1 {cx+R*.12:.2f} {cy-R*.21:.2f}" stroke="#fff" stroke-opacity=".35" stroke-width="1.2" fill="none"/>
</g>
<rect width="128" height="64" fill="url(#gloss)"/>
</g>
<path d="{SQ}" fill="none" stroke="#000" stroke-opacity=".25" stroke-width="1"/></svg>'''
open('6-single-reel-brushed.svg','w').write(svg)
