import re,subprocess
exec(open('reel.py').read().replace("open('6-single-reel-brushed.svg','w').write(svg)",""))
base=svg
# warm metal + stronger shadow
base=base.replace('<stop offset="0" stop-color="#e9ebee"/><stop offset=".55" stop-color="#c9ccd1"/><stop offset="1" stop-color="#a3a7ae"/>',
 '<stop offset="0" stop-color="#e9ebee"/><stop offset=".55" stop-color="#c6c9ce"/><stop offset="1" stop-color="#9ca0a7"/>')
base=base.replace('<feGaussianBlur in="SourceAlpha" stdDeviation="2.4"/><feOffset dy="2.4"/><feComponentTransfer><feFuncA type="linear" slope=".5"/>',
 '<feGaussianBlur in="SourceAlpha" stdDeviation="4"/><feOffset dx="1.2" dy="4.6"/><feComponentTransfer><feFuncA type="linear" slope="1.1"/>')
warm='''<radialGradient id="warm" cx=".3" cy=".2" r=".95"><stop offset="0" stop-color="#ffc878" stop-opacity=".16"/><stop offset=".6" stop-color="#ffb060" stop-opacity=".04"/><stop offset="1" stop-color="#2a1000" stop-opacity=".18"/></radialGradient>'''
base=base.replace('<linearGradient id="gloss"',warm+'<linearGradient id="gloss"')
base=base.replace('<rect width="128" height="64" fill="url(#gloss)"/>','<rect width="128" height="128" fill="url(#warm)"/><rect width="128" height="64" fill="url(#gloss)"/>')
bgseg=re.search(r'<rect width="128" height="128" fill="url\(#plate\)"/>.*?(?=<g filter="url\(#drop\)">)',base,re.S).group(0)
warmplate=bgseg.replace('fill="url(#plate)"','fill="url(#plate2)"')
plate2='<linearGradient id="plate2" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#efe8dc"/><stop offset=".55" stop-color="#cbc2b3"/><stop offset="1" stop-color="#a39886"/></linearGradient>'
tolex='''<filter id="pebble" x="0" y="0" width="100%" height="100%"><feTurbulence type="fractalNoise" baseFrequency="1.1" numOctaves="3" seed="9" result="n"/>
<feDiffuseLighting in="n" surfaceScale="1.6" lighting-color="#fff" result="l"><feDistantLight azimuth="235" elevation="55"/></feDiffuseLighting>
<feColorMatrix in="l" values=".1 0 0 0 0  0 .095 0 0 0  0 0 .09 0 0  0 0 0 1 0"/></filter>
<radialGradient id="tolexsheen" cx=".35" cy=".25" r=".8"><stop offset="0" stop-color="#ffd9a8" stop-opacity=".12"/><stop offset="1" stop-color="#000" stop-opacity=".35"/></radialGradient>'''
tolexbg='<rect width="128" height="128" fill="#111" filter="url(#pebble)"/><rect width="128" height="128" fill="#0b0b0c" opacity=".6"/><rect width="128" height="128" fill="url(#tolexsheen)"/>'
# tweed: diagonal twill
tw=''
import random; random.seed(5)
for i in range(-140,140,3):
    c=random.choice(['#a8722a','#9c6824','#b47c30'])
    tw+=f'<path d="M{i} 0 L{i+128} 128" stroke="{c}" stroke-width="1.3"/>'
for i in range(-140,140,3):
    tw+=f'<path d="M{i+1.6} 0 L{i+129} 128" stroke="#f6dc96" stroke-width=".9" stroke-opacity=".85"/>'
tweedf='''<filter id="weave" x="0" y="0" width="100%" height="100%"><feTurbulence type="fractalNoise" baseFrequency="2.2 .9" numOctaves="2" seed="2"/><feColorMatrix values="0 0 0 0 .35  0 0 0 0 .22  0 0 0 0 .05  0 0 0 .35 0"/><feComposite in2="SourceGraphic" operator="in"/></filter>
<radialGradient id="tweedsh" cx=".35" cy=".25" r=".9"><stop offset="0" stop-color="#fff2c8" stop-opacity=".18"/><stop offset="1" stop-color="#5a3800" stop-opacity=".35"/></radialGradient>'''
tweedbg=f'<rect width="128" height="128" fill="#d2a149"/><g transform="rotate(0)">{tw}</g><rect width="128" height="128" fill="#888" filter="url(#weave)"/><rect width="128" height="128" fill="url(#tweedsh)"/>'
out={'7-reel-warm-metal':base.replace(bgseg,warmplate).replace('<linearGradient id="gloss"',plate2+'<linearGradient id="gloss"'),
     '8-reel-tolex':base.replace('<rect width="128" height="128" fill="url(#warm)"/>','<rect width="128" height="128" fill="url(#warm)" opacity=".45"/>').replace('<rect width="128" height="64" fill="url(#gloss)"/>','<rect width="128" height="64" fill="url(#gloss)" opacity=".35"/>').replace(bgseg,tolexbg).replace('<linearGradient id="gloss"',tolex+'<linearGradient id="gloss"'),
     '9-reel-tweed':base.replace(bgseg,tweedbg).replace('<linearGradient id="gloss"',tweedf+'<linearGradient id="gloss"')}
for k,v in out.items(): open(k+'.svg','w').write(v)
def dark(v):
    v=v.replace('<stop offset=".3" stop-color="#2e1a0d"/><stop offset=".6" stop-color="#5a341c"/><stop offset=".85" stop-color="#6e4224"/><stop offset="1" stop-color="#3a2010"/>',
                '<stop offset=".3" stop-color="#0c0806"/><stop offset=".6" stop-color="#1c130d"/><stop offset=".85" stop-color="#2a1c13"/><stop offset="1" stop-color="#0a0605"/>')
    v=v.replace('#a0663c','#8a6a52').replace('#1c0e06','#000')
    v=v.replace('<stop offset=".5" stop-color="#fff" stop-opacity="0"/>','<stop offset=".5" stop-color="#fff" stop-opacity=".55"/>')
    v=v.replace('<stop offset="0" stop-color="#ffcf9a" stop-opacity=".5"/><stop offset=".5" stop-color="#ffb070" stop-opacity=".12"/>','<stop offset="0" stop-color="#ffe2c0" stop-opacity=".42"/><stop offset=".5" stop-color="#ffc890" stop-opacity=".08"/>')
    v=v.replace('stroke="#c88a58" stroke-opacity=".6"','stroke="#d9b08a" stroke-opacity=".8"')
    v=v.replace('<circle cx="64" cy="64" r="'+str(R*.86)+'" fill="url(#sheen)"/>','')
    return v
open('10-reel-tolex-dark-tape.svg','w').write(dark(out['8-reel-tolex']))
open('11-reel-tweed-dark-tape.svg','w').write(dark(out['9-reel-tweed']))
