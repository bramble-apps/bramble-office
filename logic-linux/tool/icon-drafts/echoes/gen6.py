SQ=open('../tape/gen.py').read().split('SQ="')[1].split('"')[0]
# reference measured: ring outer dia 80% of tile, stroke 6.7% of tile; tile = 112 units here
T=112; W=T*.067; Rout=T*.40; R=Rout-W/2
cx=64; B=64+R          # bottom tangent point (stroke centre)
rr=[R, R*.64, R*.30]
fills=['url(#d1)','url(#d2)','url(#d3)']
discs=''.join(f'<circle cx="{cx}" cy="{B-r}" r="{r}" fill="{f}"/><circle cx="{cx}" cy="{B-r}" r="{r-W/2-.3}" fill="none" stroke="#08323a" stroke-opacity=".55" stroke-width=".7"/>' for r,f in zip(rr,fills))
strokes=''.join(f'<circle cx="{cx}" cy="{B-r}" r="{r}" fill="none" stroke="url(#ring)" stroke-width="{W:.2f}"/>' for r in rr)
svg=f'''<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128"><defs>
<clipPath id="sq"><path d="{SQ}"/></clipPath>
<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#17616b"/><stop offset=".55" stop-color="#0d3b45"/><stop offset="1" stop-color="#061a21"/></linearGradient>
<linearGradient id="ring" x1=".75" y1="0" x2=".25" y2="1"><stop offset="0" stop-color="#dcfff7"/><stop offset=".45" stop-color="#93f0dc"/><stop offset="1" stop-color="#3fd3c3"/></linearGradient>
<linearGradient id="d1" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1d5a63"/><stop offset="1" stop-color="#133f48"/></linearGradient>
<linearGradient id="d2" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#236b74"/><stop offset="1" stop-color="#18505a"/></linearGradient>
<linearGradient id="d3" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#2c8590"/><stop offset="1" stop-color="#22717c"/></linearGradient>
<filter id="rimf" x="-5%" y="-5%" width="110%" height="110%"><feMorphology in="SourceAlpha" operator="erode" radius=".75" result="e"/><feComposite in="SourceAlpha" in2="e" operator="out" result="edge"/><feFlood flood-color="#fff"/><feComposite in2="edge" operator="in"/></filter>
<filter id="glowf" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur in="SourceAlpha" stdDeviation="3"/><feFlood flood-color="#5ff2d8" flood-opacity=".45"/><feComposite operator="in" in2="SourceAlpha"/><feGaussianBlur stdDeviation="2.5"/></filter>
<linearGradient id="fadeg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".95"/><stop offset=".6" stop-color="#fff" stop-opacity=".45"/><stop offset="1" stop-color="#fff" stop-opacity=".25"/></linearGradient>
<mask id="fade"><rect width="128" height="128" fill="url(#fadeg)"/></mask>
<linearGradient id="tilerimg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".45"/><stop offset=".25" stop-color="#fff" stop-opacity=".06"/><stop offset="1" stop-color="#fff" stop-opacity=".12"/></linearGradient>
<filter id="lift" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur in="SourceAlpha" stdDeviation="2"/><feOffset dy="2"/><feComponentTransfer><feFuncA type="linear" slope=".3"/></feComponentTransfer><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter>
</defs>
<g clip-path="url(#sq)">
<rect width="128" height="128" fill="url(#bg)"/>
<g filter="url(#glowf)">{strokes}</g>
<g filter="url(#lift)">{discs}{strokes}</g>
<g mask="url(#fade)" opacity=".75"><g filter="url(#rimf)">{strokes}</g></g>
<path d="{SQ}" fill="none" stroke="url(#tilerimg)" stroke-width="2"/>
</g></svg>'''
open('echoes-f.svg','w').write(svg)
