import math
SQ="M46.52,8.00 L81.48,8.00 C92.57,8.00 98.12,8.00 103.12,9.65 L104.09,9.89 C110.60,12.26 115.74,17.40 118.11,23.91 C120.00,29.88 120.00,35.43 120.00,46.52 L120.00,81.48 C120.00,92.57 120.00,98.12 118.35,103.12 L118.11,104.09 C115.74,110.60 110.60,115.74 104.09,118.11 C98.12,120.00 92.57,120.00 81.48,120.00 L46.52,120.00 C35.43,120.00 29.88,120.00 24.88,118.35 L23.91,118.11 C17.40,115.74 12.26,110.60 9.89,104.09 C8.00,98.12 8.00,92.57 8.00,81.48 L8.00,46.52 C8.00,35.43 8.00,29.88 9.89,23.91 C12.26,17.40 17.40,12.26 23.91,9.89 L24.88,9.65 C29.88,8.00 35.43,8.00 46.52,8.00 Z"
COMMON='''
<clipPath id="sq"><path d="%s"/></clipPath>
<linearGradient id="gloss" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".22"/><stop offset=".35" stop-color="#fff" stop-opacity=".04"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>
<linearGradient id="walnut" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#8a4a22"/><stop offset=".5" stop-color="#6a3414"/><stop offset="1" stop-color="#3e1c08"/></linearGradient>
<linearGradient id="alu" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#f4f5f7"/><stop offset=".45" stop-color="#b9bdc4"/><stop offset=".55" stop-color="#d9dce1"/><stop offset="1" stop-color="#8d929a"/></linearGradient>
<radialGradient id="tape" cx=".5" cy=".5" r=".5"><stop offset=".3" stop-color="#3a2416"/><stop offset=".85" stop-color="#5a3a24"/><stop offset="1" stop-color="#2a180c"/></radialGradient>
<linearGradient id="brushed" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#e9eaec"/><stop offset=".5" stop-color="#c3c6cb"/><stop offset="1" stop-color="#9ea2a9"/></linearGradient>
<linearGradient id="dark" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#3a3b40"/><stop offset="1" stop-color="#16171a"/></linearGradient>
<linearGradient id="cream" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fbf4e2"/><stop offset="1" stop-color="#e9dcbc"/></linearGradient>
<radialGradient id="red" cx=".4" cy=".35" r=".7"><stop offset="0" stop-color="#ff8a7a"/><stop offset=".5" stop-color="#e5302a"/><stop offset="1" stop-color="#8f1210"/></radialGradient>
<filter id="drop" x="-20%%" y="-20%%" width="140%%" height="140%%"><feGaussianBlur in="SourceAlpha" stdDeviation="1.6"/><feOffset dy="1.6"/><feComponentTransfer><feFuncA type="linear" slope=".45"/></feComponentTransfer><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter>
'''%SQ
def frame(bg,body):
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128"><defs>{COMMON}</defs>
<g clip-path="url(#sq)">{bg}{body}<rect width="128" height="60" fill="url(#gloss)"/></g>
<path d="{SQ}" fill="none" stroke="#000" stroke-opacity=".25" stroke-width="1"/></svg>'''
def grain(y0=8,y1=120,col="#2e1204"):
    import random; random.seed(3); s=''
    y=y0
    while y<y1:
        s+=f'<path d="M0 {y:.1f} C40 {y+random.uniform(-1.5,1.5):.1f} 80 {y+random.uniform(-1.5,1.5):.1f} 128 {y:.1f}" stroke="{random.choice([col,"#c07040"])}" stroke-width="{random.uniform(.4,1.1):.2f}" fill="none" opacity="{random.uniform(.1,.3):.2f}"/>'
        y+=random.uniform(1.5,3.5)
    return s
def reel(cx,cy,r,tape_r,spokes=3,hub="#2a2b2f"):
    s=f'<circle cx="{cx}" cy="{cy}" r="{tape_r}" fill="url(#tape)"/>' if tape_r else ''
    s+=f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="url(#alu)" opacity=".0"/>'
    # flange with cut-outs
    s+=f'<g filter="url(#drop)"><circle cx="{cx}" cy="{cy}" r="{r}" fill="url(#alu)" fill-opacity=".92"/>'
    for k in range(spokes):
        a=k*2*math.pi/spokes-math.pi/2
        x=cx+math.cos(a)*r*.58; y=cy+math.sin(a)*r*.58
        s+=f'<ellipse cx="{x:.2f}" cy="{y:.2f}" rx="{r*.26:.2f}" ry="{r*.26:.2f}" fill="url(#tape)"/>'
    s+=f'<circle cx="{cx}" cy="{cy}" r="{r*.2:.2f}" fill="{hub}"/><circle cx="{cx}" cy="{cy}" r="{r*.08:.2f}" fill="#bbb"/>'
    s+=f'<circle cx="{cx}" cy="{cy}" r="{r-.5}" fill="none" stroke="#fff" stroke-opacity=".5" stroke-width=".8"/></g>'
    return s

icons={}
# 1 cassette on charcoal
cas='''<g filter="url(#drop)" transform="translate(0,2)">
<rect x="16" y="30" width="96" height="62" rx="5" fill="url(#dark)"/>
<rect x="16.5" y="30.5" width="95" height="61" rx="4.5" fill="none" stroke="#fff" stroke-opacity=".15"/>
<rect x="22" y="35" width="84" height="38" rx="2.5" fill="url(#cream)"/>
<rect x="22" y="35" width="84" height="7" fill="#e5302a"/><rect x="22" y="42" width="84" height="3" fill="#f2a32a"/>
<path d="M27 50 H74 M27 55 H62" stroke="#8a7a5a" stroke-width="1" opacity=".6"/>
<rect x="38" y="57" width="52" height="13" rx="6.5" fill="#1a1a1d"/>
<circle cx="47" cy="63.5" r="6" fill="#3a2416"/><circle cx="81" cy="63.5" r="4.2" fill="#3a2416"/>
<circle cx="47" cy="63.5" r="3" fill="#eee"/><circle cx="81" cy="63.5" r="3" fill="#eee"/>
<rect x="56" y="59" width="16" height="9" fill="#4a3a2e" opacity=".7"/>
<path d="M30 92 L36 78 H92 L98 92 Z" fill="#2a2b30"/><circle cx="44" cy="86" r="2" fill="#111"/><circle cx="84" cy="86" r="2" fill="#111"/>
<circle cx="64" cy="86" r="1.6" fill="#111"/></g>'''
icons['1-cassette']=frame('<rect width="128" height="128" fill="#26272b"/><rect width="128" height="128" fill="url(#gloss)" opacity=".4"/>',cas)
# 2 reel-to-reel deck on brushed metal
deck='<rect width="128" height="128" fill="url(#brushed)"/>'+''.join(f'<path d="M0 {y} H128" stroke="#fff" stroke-opacity=".18" stroke-width=".4"/>' for y in range(8,121,2))
r2=reel(40,48,22,20,3)+reel(88,48,22,12,3)
r2+='<path d="M40 70 L52 92 H76 L88 70" stroke="#3a2416" stroke-width="1.4" fill="none"/>'
r2+='<rect x="44" y="88" width="40" height="10" rx="2" fill="#2a2b30"/><rect x="56" y="90" width="16" height="6" rx="1" fill="#6b6e75"/>'
r2+='<rect x="18" y="104" width="92" height="10" rx="2" fill="#2a2b30"/>'+''.join(f'<rect x="{22+i*15}" y="106" width="12" height="6" rx="1" fill="#8d929a"/>' for i in range(5))+'<circle cx="103" cy="109" r="3" fill="url(#red)"/>'
icons['2-reel-to-reel']=frame(deck,r2)
# 3 single big reel on walnut
wal='<rect width="128" height="128" fill="url(#walnut)"/>'+grain()
r3=reel(64,62,44,40,3,hub="#1d1e21")
r3+='<path d="M64 106 C80 112 100 112 120 108" stroke="#3a2416" stroke-width="3" fill="none"/>'
icons['3-open-reel-walnut']=frame(wal,r3)
# 4 VU meter + record button on walnut/black faceplate
vu=wal
v='<g filter="url(#drop)"><rect x="16" y="20" width="96" height="62" rx="5" fill="url(#dark)"/><rect x="22" y="25" width="84" height="52" rx="3" fill="url(#cream)"/>'
for k in range(11):
    a=math.radians(-50+k*10); x1=64+math.sin(a)*34; y1=74-math.cos(a)*34; x2=64+math.sin(a)*(30 if k%2 else 28); y2=74-math.cos(a)*(30 if k%2 else 28)
    v+=f'<path d="M{x1:.1f} {y1:.1f} L{x2:.1f} {y2:.1f}" stroke="{"#d22" if k>7 else "#333"}" stroke-width="1"/>'
v+='<path d="M{0:.1f} {1:.1f} A34 34 0 0 1 {2:.1f} {3:.1f}" stroke="#333" stroke-width=".8" fill="none"/>'.format(64+math.sin(math.radians(-50))*34,74-math.cos(math.radians(-50))*34,64+math.sin(math.radians(20))*34,74-math.cos(math.radians(20))*34)
v+='<path d="M{0:.1f} {1:.1f} A34 34 0 0 1 {2:.1f} {3:.1f}" stroke="#d22" stroke-width="2" fill="none"/>'.format(64+math.sin(math.radians(20))*34,74-math.cos(math.radians(20))*34,64+math.sin(math.radians(50))*34,74-math.cos(math.radians(50))*34)
v+='<text x="38" y="70" font-family="sans-serif" font-size="7" font-weight="700" text-anchor="start" fill="#333">VU</text>'
v+=f'<path d="M64 74 L{64+math.sin(math.radians(12))*40:.1f} {74-math.cos(math.radians(12))*40:.1f}" stroke="#111" stroke-width="1.3"/><circle cx="64" cy="74" r="3" fill="#222"/></g>'
v+='<g filter="url(#drop)"><circle cx="64" cy="100" r="11" fill="#1d1e21"/><circle cx="64" cy="100" r="8.5" fill="url(#red)"/></g>'
icons['4-vu-meter-record']=frame(vu,v)
# 5 tape loop + record dot on graphite gradient: stylised tape ribbon forming a waveform
g5='<defs><linearGradient id="g5" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#2f3a4a"/><stop offset="1" stop-color="#121820"/></linearGradient><linearGradient id="rib" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#6b4226"/><stop offset=".5" stop-color="#a86a3a"/><stop offset="1" stop-color="#6b4226"/></linearGradient></defs><rect width="128" height="128" fill="url(#g5)"/>'
b5=reel(34,50,20,18,5)+reel(94,50,20,10,5)
b5+='<path d="M16 50 C16 92 40 98 64 82 S108 70 112 50" stroke="url(#rib)" stroke-width="4" fill="none" filter="url(#drop)"/>'
b5+='<g filter="url(#drop)"><circle cx="64" cy="100" r="9" fill="url(#red)"/></g>'
icons['5-twin-reels-record']=frame(g5,b5)
for k,v in icons.items(): open(k+'.svg','w').write(v)
print(list(icons))
