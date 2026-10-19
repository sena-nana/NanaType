"""Build original SVG masters. Visual references/rights: docs/skin-art-sources.md.

The masters are deliberately ordinary SVG paths, not runtime scripts. Rasterize
with render-skin-ornaments.cjs; the skin renderer only loads the resulting PNG.
"""
from math import sin, cos, pi
from pathlib import Path
import random

ROOT = Path(__file__).resolve().parents[2] / "crates/theme/builtin"


class Drawing:
    def __init__(self, width=256):
        self.width = width
        self.parts = []

    def add(self, value):
        self.parts.append(value)

    def path(self, d, fill="none", stroke="none", width=1, opacity=1):
        self.add(f'<path d="{d}" fill="{fill}" stroke="{stroke}" stroke-width="{width}" opacity="{opacity}" stroke-linecap="round" stroke-linejoin="round"/>')

    def circle(self, x, y, r, fill="none", stroke="none", width=1, opacity=1):
        self.add(f'<circle cx="{x:.2f}" cy="{y:.2f}" r="{r:.2f}" fill="{fill}" stroke="{stroke}" stroke-width="{width}" opacity="{opacity}"/>')

    def group(self, transform, opacity=1):
        self.add(f'<g transform="{transform}" opacity="{opacity}">')

    def end(self):
        self.add('</g>')

    def save(self, name):
        svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="{self.width}" height="320" viewBox="0 0 {self.width} 320">
<title>NanaType {name} ornament</title>
<desc>Original vector illustration, CC0-1.0. Reference register: docs/skin-art-sources.md.</desc>
<defs>
 <linearGradient id="gold" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#F1DDA8"/><stop offset=".48" stop-color="#C7AB76"/><stop offset="1" stop-color="#A97E4A"/></linearGradient>
 <radialGradient id="wine"><stop stop-color="#B44C69"/><stop offset="1" stop-color="#672039"/></radialGradient>
 <linearGradient id="leaf" x1="0" y1="1" x2="1" y2="0"><stop stop-color="#41624D"/><stop offset=".55" stop-color="#739276"/><stop offset="1" stop-color="#A6B79A"/></linearGradient>
 <radialGradient id="bell" cx=".44" cy=".2" r=".85"><stop stop-color="#D7F9ED" stop-opacity=".48"/><stop offset=".65" stop-color="#79D4CE" stop-opacity=".25"/><stop offset="1" stop-color="#79D4CE" stop-opacity=".08"/></radialGradient>
 <linearGradient id="silk"><stop stop-color="#8D282D"/><stop offset=".32" stop-color="#C45045"/><stop offset=".55" stop-color="#D4624A"/><stop offset="1" stop-color="#9D2D30"/></linearGradient>
 <radialGradient id="juice"><stop stop-color="#FFDD74"/><stop offset=".72" stop-color="#F8C34D"/><stop offset="1" stop-color="#F0AF36"/></radialGradient>
</defs>
''' + '\n'.join(self.parts) + '\n</svg>\n'
        (ROOT / name / "ornament.svg").write_text(svg, encoding="utf-8")


def gothic():
    d = Drawing(512)
    d.group("translate(128 160)")
    for r, w, color in [(94, 2.2, '#C7AB76'), (89, .8, '#E7CC94'), (84, 2.7, '#9D7A4D')]:
        d.circle(0, 0, r, stroke=color, width=w)
    for n in range(12):
        d.group(f"rotate({n*30})")
        d.path('M0 -20 C-15 -34 -25 -54 0 -80 C25 -54 15 -34 0 -20Z', 'url(#wine)', '#D9BB80', 1.7)
        d.path('M0 -29 C-10 -43 -14 -55 0 -69 C14 -55 10 -43 0 -29Z', 'none', '#E7CC94', .7, .68)
        d.path('M0 -22 L0 -76', stroke='#D9BB80', width=.65, opacity=.48)
        d.circle(0, -86, 2, '#E7CC94')
        d.path('M0 -101 C-10 -111 -8 -118 0 -123 C8 -118 10 -111 0 -101Z', 'none', '#C7AB76', 1.2)
        d.end()
    d.circle(0, 0, 23, '#512035', 'url(#gold)', 2)
    for n in range(5):
        d.group(f'rotate({n*72})')
        d.path('M0 3 C-21 -2 -21 -18 -6 -18 C3 -19 10 -9 0 3Z', '#A84360', '#E1B48D', .85)
        d.end()
    d.path('M-8 0 C-9 -10 7 -11 8 -2 C10 6 -1 10 -5 4 C-9 -1 -2 -6 2 -3', stroke='#F0D4B0', width=1)
    d.end()
    for flip, y in [(1, 15), (-1, 305)]:
        d.group(f'translate(128 {y}) scale(1 {flip})')
        d.path('M0 0 L0 16 M-3 18 L0 23 L3 18 L0 13Z', '#C7AB76', '#C7AB76', .7)
        d.path('M-6 27 C-25 10 -43 18 -39 32 C-35 43 -20 37 -27 28 M6 27 C25 10 43 18 39 32 C35 43 20 37 27 28', stroke='#C7AB76', width=1.5)
        d.end()
    # Second atlas half: real pointed heads, inset mullions and quatrefoils.
    d.group('translate(256 0)')
    for x, y, scale in [(82, 39, 1.), (174, 64, .78)]:
        d.group(f'translate({x} {y}) scale({scale})')
        d.path('M-43 231 V82 Q-43 32 0 0 Q43 32 43 82 V231Z', 'none', 'url(#gold)', 2.1)
        d.path('M-35 224 V84 Q-35 42 0 12 Q35 42 35 84 V224Z', 'none', '#C7AB76', .8)
        d.path('M0 108 V223 M-34 223 V148 Q-34 123 -17 108 Q0 123 0 148 M0 148 Q0 123 17 108 Q34 123 34 148 V223 M-34 223 H34', stroke='#C7AB76', width=1.1)
        d.group('translate(0 78)')
        for n in range(4):
            d.group(f'rotate({n*90})')
            d.path('M0 0 C-16 -8 -10 -29 0 -25 C10 -29 16 -8 0 0Z', stroke='#C7AB76', width=1.1)
            d.end()
        d.end()
        d.path('M-38 103 H38 M-35 209 H35', stroke='#C7AB76', width=.65, opacity=.6)
        d.end()
    d.end()
    d.save('gothic')


def celestial():
    d = Drawing()
    d.group('translate(128 160)')
    for r, w, op in [(117, 1.5, .85), (112, .6, .7), (103, 1.4, .9), (97, .65, .65), (73, .65, .5)]:
        d.circle(0, 0, r, stroke='#E7CC94', width=w, opacity=op)
    for n in range(72):
        a = n*pi/36; r = 104 if n%6 == 0 else 108 if n%3 == 0 else 110
        d.path(f'M{r*cos(a):.2f} {r*sin(a):.2f} L{115*cos(a):.2f} {115*sin(a):.2f}', stroke='#E7CC94', width=1 if n%6==0 else .6, opacity=.8)
    # Engraved arcs and openwork rete, with a quiet clear field for the moon.
    for x, y, r in [(0, 51, 88), (0, -51, 88), (61, 0, 84), (-61, 0, 84)]:
        d.path(f'M{-72 if x==0 else -43} {y} Q{x} {-104 if y>0 else 104} {72 if x==0 else 43} {y}', stroke='#B5A1E8', width=.65, opacity=.4)
    d.add('<ellipse cx="0" cy="4" rx="92" ry="40" transform="rotate(-25)" fill="none" stroke="#E7CC94" stroke-width="1.1" opacity=".75"/>')
    d.add('<ellipse cx="-12" cy="-7" rx="79" ry="51" transform="rotate(57)" fill="none" stroke="#B5A1E8" stroke-width=".8" opacity=".65"/>')
    d.path('M-102 0 H102 M0 -101 V101', stroke='#B5A1E8', width=.55, opacity=.35)
    d.path('M17 -36 C-27 -55 -54 -3 -22 27 C-6 42 16 39 30 25 C-17 39 -34 -16 17 -36Z', 'url(#gold)')
    d.path('M-17 -31 C-35 -16 -35 6 -20 21', stroke='#FFF0CC', width=.85, opacity=.7)
    for x,y,k in [(-60,-56,4),(65,48,3.2),(45,-71,2.5),(-68,47,2.8),(84,-22,2)]:
        d.path(f'M{x-k} {y} L{x} {y-k*1.5} L{x+k} {y} L{x} {y+k*1.5}Z', '#E7CC94')
    d.path('M-61 -56 L45 -71 L84 -22 L65 48 L-68 47', stroke='#E7CC94', width=.6, opacity=.3)
    d.circle(0, 0, 3.3, '#E7CC94')
    d.end()
    d.path('M116 41 Q128 29 140 41 M123 29 V18 Q128 9 133 18 V29', stroke='#E7CC94', width=1.2, opacity=.7)
    d.save('celestial')


def forest():
    d = Drawing()

    def point(points, t):
        return tuple((1-t)**3*a+3*(1-t)**2*t*b+3*(1-t)*t*t*c+t**3*e
                     for a,b,c,e in zip(*points))

    def pinna(length, shade):
        # One continuous scalloped pinna, rather than a pile of bead-like leaves.
        n=7
        def centre(u): return -.37*length*u-.055*length*sin(pi*u)
        def half(u): return (3.+length*.075)*max(0.,sin(pi*u))**.8
        path='M0 0 '
        for side, order in [(-1, range(n)), (1, range(n-1,-1,-1))]:
            for j in order:
                u=(j+1)/n if side<0 else j/n
                mid=(j+.5)/n
                path+=f'Q{length*mid:.2f} {centre(mid)+side*half(mid)*1.2:.2f} {length*u:.2f} {centre(u)+side*half(u)*.7:.2f} '
        d.path(path+'Z',shade,'#526F54',.38)
        d.path(f'M0 0 Q{length*.5:.2f} {centre(.5):.2f} {length:.2f} {centre(1):.2f}',stroke='#B2C3A2',width=.55,opacity=.72)
        for j in range(1,n):
            u=j/n
            for side in [-1,1]:
                d.path(f'M{length*u:.2f} {centre(u):.2f} l{length*.045:.2f} {side*half(u)*.68:.2f}',stroke='#B2C3A2',width=.3,opacity=.65)

    # A small rear frond has its own stem and wider breathing room at the root.
    rear=[(85,296),(72,264),(45,238),(46,198)]
    d.group('',.62)
    d.path('M85 296 C72 264 45 238 46 198',stroke='#5D795C',width=1.1)
    for j in range(7):
        t=(j+1)/9; x,y=point(rear,t)
        for side in [-1,1]:
            d.group(f'translate({x:.2f} {y:.2f}) scale({side} 1)')
            pinna(19*sin(pi*t)**.8,'#8CA181')
            d.end()
    d.path('M48 219 Q39 206 46 195 Q53 207 48 219Z','#8CA181','#526F54',.35)
    d.end()
    main=[(68,299),(89,232),(142,113),(155,26)]
    d.path('M68 299 C89 232 142 113 155 26',stroke='#49674F',width=1.6)
    d.path('M69 296 C90 229 142 111 155 28',stroke='#A5B995',width=.45,opacity=.8)
    for j in range(13):
        t=(j+1)/14; x,y=point(main,t)
        for side in [-1,1]:
            # Alternate insertion and length instead of mirrored, identical fans.
            shift=2.1 if side>0 else -1.1
            span=62*sin(pi*t)**.9*(1-.30*t)*(1+.06*sin(j*1.7+side))
            d.group(f'translate({x:.2f} {y+shift:.2f}) scale({side} 1)')
            pinna(span,'url(#leaf)')
            d.end()
    d.path('M148 49 C147 38 151 27 157 20 C160 33 158 42 148 49Z','url(#leaf)','#526F54',.4)
    d.path('M149 47 L157 22',stroke='#B2C3A2',width=.4,opacity=.8)
    # A tapering, still-unfurling crozier and two matte specimen-paper ties.
    d.path('M88 299 C133 272 177 235 185 207 C193 183 174 177 166 188 C156 202 179 210 180 196',stroke='#799372',width=1.3)
    d.path('M180 196 C177 190 169 191 170 198',stroke='#A0B28F',width=.65)
    d.path('M178 185 Q171 176 168 183 Q170 190 177 191Z','#8CA181','#668260',.4)
    d.path('M62 286 L91 291 L90 296 L62 291Z','#D5C89B','#A79D73',.45,.95)
    d.path('M64 296 L86 298',stroke='#BBAF83',width=2.4,opacity=.8)
    d.save('forest')


def tidal():
    d = Drawing()
    d.path('M43 124 C45 71 80 36 126 37 C175 35 211 76 214 123 C199 113 199 138 181 127 C165 113 163 139 145 128 C126 115 123 141 105 128 C86 114 83 139 66 127 C55 118 52 132 43 124Z','url(#bell)','#A4E6DE',1.7)
    d.path('M50 111 C62 68 87 49 123 47 C160 43 190 71 205 109',stroke='#EEF9F6',width=.8,opacity=.6)
    for j in range(9):
        x=51+j*19
        d.path(f'M126 46 C{110+(x-126)*.18:.1f} 64 {x} 82 {x} 120',stroke='#B9EEE4',width=.7,opacity=.52)
    # Radial canals, four translucent gonads and a scalloped marginal fringe.
    for x,y in [(110,83),(141,84),(112,104),(142,104)]:
        d.path(f'M{x-8} {y} C{x-12} {y-13} {x+9} {y-16} {x+9} {y-3} C{x+13} {y+10} {x-8} {y+13} {x-8} {y}Z','none','#B9EEE4',1.2,.64)
    for j in range(21):
        x=47+j*8
        d.path(f'M{x} 126 q-3 8 1 12',stroke='#91DED6',width=.65,opacity=.7)
    for j in range(12):
        x=54+j*13; length=91+34*sin(j*1.8)
        d.path(f'M{x} 131 C{x-24} 170 {x+25} 185 {x-8} {131+length:.1f} C{x-22} {157+length:.1f} {x+19} {151+length:.1f} {x+7} {168+length:.1f}',stroke='#88D6D0',width=.7,opacity=.55)
    for x,flip in [(103,1),(146,-1)]:
        d.path(f'M{x} 129 C{x-18*flip} 160 {x+27*flip} 183 {x+2*flip} 215 C{x-22*flip} 247 {x+11*flip} 276 {x-9*flip} 296',stroke='#B2EADF',width=7,opacity=.13)
        d.path(f'M{x} 129 C{x-18*flip} 160 {x+27*flip} 183 {x+2*flip} 215 C{x-22*flip} 247 {x+11*flip} 276 {x-9*flip} 296',stroke='#D4F5E9',width=1.1,opacity=.75)
        for y in range(154,267,14):
            d.path(f'M{x} {y} q{-15*flip} 4 {-4*flip} 12 q{17*flip} 5 {3*flip} 13',stroke='#B2EADF',width=.85,opacity=.65)
    d.save('tidal')


def lantern():
    d = Drawing()
    d.path('M128 5 V37',stroke='#B83B38',width=1.9)
    d.path('M126 7 V34',stroke='#DCB875',width=.45,opacity=.7)
    d.path('M98 41 Q128 28 158 41 L166 54 H90Z','url(#gold)','#916C3E',1.2)
    d.path('M92 52 H164 V62 H92Z','url(#gold)','#A47B43',.9)
    body='M94 58 Q75 66 63 84 L56 168 Q56 182 86 208 H170 Q200 182 200 168 L193 84 Q181 66 162 58Z'
    d.add(f'<defs><clipPath id="lamp-body"><path d="{body}"/></clipPath></defs>')
    d.path(body,'url(#silk)','#C6A15B',1.8)
    d.add('<g clip-path="url(#lamp-body)">')
    d.path('M61 81 L88 58 L78 199 L55 172Z','#6F242B',opacity=.22)
    d.path('M168 58 L195 81 L202 173 L178 198Z','#6F242B',opacity=.25)
    d.path('M103 59 H150 L157 198 H100Z','#F2AB6B',opacity=.08)
    # Quiet silk weave, subordinate to the silhouette and the warm panel ribs.
    for y in range(88,171,5):
        d.path(f'M57 {y} H199',stroke='#F6D59C',width=.35,opacity=.08)
    for x in [78,101,128,155,178]:
        d.path(f'M{x} 66 Q{128+(x-128)*1.45:.1f} 134 {x} 201',stroke='#E5BD77',width=.85,opacity=.75)
    d.path('M64 82 L192 82 M57 175 H199 M86 201 H170',stroke='#D5B372',width=1.1)
    for x in range(68,185,13):
        d.path(f'M{x} 87 q3 -5 6 -3 q5 3 0 5 h-4',stroke='#E5BD77',width=.55,opacity=.62)
        d.path(f'M{x} 180 q3 5 6 3 q5 -3 0 -5 h-4',stroke='#E5BD77',width=.55,opacity=.55)
    d.path('M81 170 C115 169 127 139 143 107 Q150 96 163 94 M115 155 Q106 145 94 145 M134 124 Q151 133 163 127',stroke='#F0CA88',width=1.15,opacity=.65)
    for x,y,k in [(129,123,1),(151,103,.63),(107,155,.69)]:
        d.group(f'translate({x} {y}) scale({k})')
        for n in range(5):
            d.group(f'rotate({n*72})')
            d.path('M0 1 C-9 -1 -12 -10 -5 -13 C2 -17 9 -8 0 1Z','#F1D69E','#DDAE64',.4,.8)
            d.path('M0 0 L-1 -7',stroke='#AD643E',width=.45,opacity=.6)
            d.end()
        d.circle(0,0,1.8,'#9B3732')
        for n in range(6):
            d.circle(cos(n*pi/3)*3,sin(n*pi/3)*3,.65,'#FFF0BA')
        d.end()
    d.end()
    d.path('M86 205 H170 L162 214 H94Z','url(#gold)','#916C3E',.9)
    d.path('M94 212 H162 L155 222 H101Z','url(#gold)','#916C3E',.9)
    d.path('M98 216 H158',stroke='#F3DCA1',width=.65,opacity=.8)
    d.path('M128 222 V244',stroke='#B83B38',width=1.5)
    d.circle(128,241,4.3,'url(#gold)','#A27C48',.6)
    d.path('M128 246 l-8 10 8 10 8 -10Z','#B83B38','#D0A45A',1)
    d.path('M128 249 l-5 7 5 7 5 -7Z',stroke='#E2B674',width=.6,opacity=.8)
    d.path('M121 265 Q128 261 135 265 V272 H121Z','#A82D32','#C6A15B',.7)
    d.path('M122 267 H134 M122 270 H134',stroke='#E5BD77',width=.55)
    for j in range(13):
        x=122+j
        d.path(f'M{x:.1f} 272 C{x-1.5:.1f} 282 {x+sin(j*.8)*3:.1f} 297 {x+sin(j*.7)*3.4:.1f} {307+cos(j*1.2)*4:.1f}',stroke='#B83B38' if j%3 else '#D18559',width=.65,opacity=.9)
    d.save('lantern')


def citrus():
    d=Drawing();rng=random.Random(1729)
    # A sliced lemon with uneven segment membranes and hundreds of juice sacs.
    d.path('M145 88 C164 49 205 35 226 48 C216 77 186 99 145 88Z','url(#leaf)','#4E8D75',1.5)
    d.path('M146 88 Q182 61 214 50',stroke='#C4D9A8',width=1)
    for n in range(7):
        x=158+n*8
        d.path(f'M{x} {83-(x-158)*.6:.1f} q10 -13 11 -24',stroke='#B2D0A3',width=.55,opacity=.7)
    d.circle(128,181,99,'#F3AD3F','#D88B2D',1)
    d.circle(128,181,94,'#FFF3C8')
    d.circle(128,181,84,'url(#juice)','#F0C369',.8)
    d.group('translate(128 181)')
    for j in range(10):
        angle=j*36+rng.uniform(-2.1,2.1)
        d.group(f'rotate({angle})')
        d.path('M-1 -8 C-5 -35 -17 -61 -19 -79 M1 -8 C8 -40 12 -64 20 -79',stroke='#FFF6D1',width=1.8)
        for n in range(13):
            y=-rng.uniform(21,77);x=rng.uniform(-.23,.23)*(-y);w=rng.uniform(.8,1.8);h=rng.uniform(2.4,4.7)
            d.path(f'M{x:.2f} {y+h:.2f} q{-w:.2f} {-h:.2f} 0 {-h*2:.2f} q{w*1.5:.2f} {h:.2f} 0 {h*2:.2f}Z',fill='#FFE693',stroke='#E5AE3E',width=.3,opacity=.85)
        if j in [1,4,7]:
            d.path('M5 -32 C-2 -34 -2 -46 3 -48 C8 -45 10 -36 5 -32Z','#FFF7DB','#C89F57',.65)
        d.end()
    d.circle(0,0,8,'#FFF4CE')
    d.path('M-6 -3 Q0 4 5 -4 M-3 5 L2 -6',stroke='#E3C180',width=.6)
    d.end()
    for j in range(90):
        a=rng.random()*pi*2;r=rng.uniform(95,98)
        d.circle(128+cos(a)*r,181+sin(a)*r,rng.uniform(.35,.8),'#F9D27E',opacity=.8)
    d.save('citrus')


for build in [gothic,celestial,forest,tidal,lantern,citrus]:
    build()
