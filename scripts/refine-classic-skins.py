"""Author the classic collection's original procedural plates and SVG masters.

Run once to update manifests, then bake still.png with the skin_artwork example.
Idempotent: preserves typography, layout, histories, Echo menus and provenance.
No source material is downloaded; the two vector masters are original CC0 art.
"""
from pathlib import Path
import re
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
BUILTIN = ROOT / 'crates/theme/builtin'
NAMES = ['halo', 'night', 'parchment', 'adv', 'rpg', 'vanilla', 'bubble',
         'livechat', 'comic', 'terminal', 'pixel', 'sakura', 'ink',
         'echo_vanilla', 'echo_glass', 'echo_bubble']
ANIMATED = {'night', 'rpg', 'bubble', 'livechat', 'terminal', 'pixel', 'sakura'}
# Preserve the actual border, header and corner artwork when a static window
# grows. These are bitmap cuts, not extra layout padding or shader parameters.
SLICES = {
    'halo': [34, 34, 34, 34], 'night': [32, 110, 22, 28],
    'parchment': [60, 152, 60, 152], 'adv': [52, 180, 22, 56],
    'rpg': [18, 24, 18, 24], 'vanilla': [44, 70, 18, 26],
    'bubble': [32, 74, 36, 34], 'livechat': [40, 82, 22, 24],
    'comic': [52, 86, 32, 32], 'terminal': [28, 28, 16, 54],
    'pixel': [18, 50, 18, 20], 'sakura': [44, 142, 30, 30],
    'ink': [34, 90, 24, 64], 'echo_vanilla': [10, 24, 10, 100],
    'echo_bubble': [28, 90, 28, 28],
}

def save_text(path, value):
    if not path.exists() or path.read_text(encoding='utf-8') != value:
        path.write_text(value, encoding='utf-8')

COMMON = '''// Original NanaType plate: stationary reading field and silhouette.
// Animate the skin's own objects, never a generic edge sweep or the text.
fn aa(d:f32)->f32 { return 1.-smoothstep(-.65,.65,d); }
fn line(d:f32,w:f32)->f32 { return aa(abs(d)-w); }
fn disc(p:vec2<f32>,r:f32)->f32 { return aa(length(p)-r); }
fn ring(p:vec2<f32>,r:f32,w:f32)->f32 { return line(length(p)-r,w); }
fn rect(p:vec2<f32>,b:vec2<f32>,r:f32)->f32 {
 let q=abs(p)-b+r; return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-r;
}
fn segment(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>,w:f32)->f32 {
 let v=b-a; return aa(length(p-a-v*clamp(dot(p-a,v)/max(dot(v,v),.01),0.,1.))-w);
}
fn rgb(r:f32,g:f32,b:f32)->vec3<f32> { return pow(vec3(r,g,b)/255.,vec3(2.2)); }
fn grain(p:vec2<f32>)->f32 { return fract(sin(dot(floor(p),vec2(12.9898,78.233)))*43758.5453); }
fn decal(p:vec2<f32>,origin:vec2<f32>,extent:vec2<f32>)->vec4<f32> {
 let uv=(p-origin)/extent;
 if(any(uv<vec2(0.)) || any(uv>vec2(1.))) { return vec4(0.); }
 return sample0(uv);
}
fn effect(i:EffectInput)->vec4<f32> {
 let p=i.uv*i.size; let s=i.size; let t=i.phase*6.283185;
 let q=p-s*.5; let b=s*.5-vec2(1.);
 let opening=exp(-i.line_age*3.); let speaker=exp(-i.speaker_age*3.);
'''

PAPER = '''// Smooth, fixed-seed paper variation at several physical scales.
fn paper_noise(p:vec2<f32>)->f32 {
 let a=floor(p); let f=fract(p); let u=f*f*(3.-2.*f);
 return mix(mix(grain(a),grain(a+vec2(1.,0.)),u.x),
            mix(grain(a+vec2(0.,1.)),grain(a+vec2(1.,1.)),u.x),u.y);
}
'''

BODIES = {
'halo': '''// Clean capsule card: air, soft blue-white and one sky-blue name pill.
// No orbit, brackets, side rail or autonomous decoration.
 let edge=rect(q,b,30.);
 var c=mix(rgb(255.,255.,255.),rgb(239.,249.,255.),i.uv.y*.8);
 c=mix(c,rgb(208.,233.,249.),line(edge+1.,.55)*.70);
 return vec4(c,aa(edge));
''',
'night': '''// A data packet follows the actual etched PCB route, then docks.
 let edge=max(rect(q,b,3.),abs(q.x)+abs(q.y)-b.x-b.y+13.);
 var c=mix(rgb(10.,18.,32.),rgb(19.,33.,53.),1.-i.uv.y);
 c=mix(c,rgb(92.,184.,255.),line(edge+1.,.7)*.8);
 c=mix(c,rgb(47.,83.,119.),line(edge+7.,.4)*.8);
 let a=vec2(s.x-102.,12.); let z=vec2(s.x-54.,21.);
 let trace=max(segment(p,a,vec2(s.x-72.,12.),.65),max(segment(p,vec2(s.x-72.,12.),z,.65),segment(p,z,vec2(s.x-18.,21.),.65)));
 c=mix(c,rgb(92.,184.,255.),trace*.65);
 c=mix(c,rgb(180.,149.,245.),ring(p-a,2.6,.6)*.8);
 let travel=clamp((i.phase-.12)/.56,0.,1.);
 var packet=mix(a,vec2(s.x-72.,12.),clamp(travel*3.,0.,1.));
 if(travel>1./3.) { packet=mix(vec2(s.x-72.,12.),z,clamp(travel*3.-1.,0.,1.)); }
 if(travel>2./3.) { packet=mix(z,vec2(s.x-22.,21.),clamp(travel*3.-2.,0.,1.)); }
 let envelope=smoothstep(.10,.15,i.phase)*(1.-smoothstep(.73,.82,i.phase));
 c=mix(c,rgb(181.,230.,255.),disc(p-packet,1.75)*envelope);
 c=mix(c,rgb(92.,184.,255.),ring(p-vec2(s.x-22.,21.),2.8,.55)*.65);
 let bracket=max(segment(p,vec2(19.,2.),vec2(19.,18.),1.),segment(p,vec2(19.,18.),vec2(34.,18.),1.));
 c=mix(c,rgb(128.,210.,255.),bracket*(.75+.2*speaker));
 return vec4(c,aa(edge));
''',
'parchment': '''// Warm, irregular paper pulp: mottling, fibres, flecks and edge wicking.
 let edge=rect(q,b,5.);
 let cloud=paper_noise(p/42.)*.65+paper_noise(p/113.+vec2(7.,13.))*.35;
 let pulp=paper_noise(p/9.);
 let fibres=paper_noise(p*vec2(.55,.065));
 let cross=paper_noise(p*vec2(.075,.7));
 let margin=min(min(p.x,s.x-p.x),min(p.y,s.y-p.y));
 let wicked=exp(-max(margin,0.)/14.)*(.45+.55*paper_noise(p/16.));
 var c=mix(rgb(249.,235.,204.),rgb(223.,194.,143.),cloud*.65);
 c*=.95+.065*pulp+.035*fibres+.018*cross;
 c=mix(c,rgb(140.,91.,43.),wicked*.27);
 c=mix(c,rgb(133.,94.,50.),step(.983,grain(p))*.09);
 c=mix(c,rgb(255.,249.,229.),step(.985,grain(p+vec2(97.,31.)))*.18);
 // Two quiet impressed fillets, with a broken ivory lip rather than a
 // polished raised rim. Four corner sprigs grow out of the inner fillet.
 let impression=.76+.18*paper_noise(p/3.);
 c=mix(c,rgb(128.,89.,48.),line(edge+1.8,.48)*.72*impression);
 c=mix(c,rgb(154.,117.,73.),line(edge+8.,.35)*.56*impression);
 c=mix(c,rgb(255.,245.,218.),line(edge+2.7,.32)*.42);
 let tl=decal(p,vec2(4.,2.),vec2(144.,56.));
 let tr=decal(vec2(s.x-p.x,p.y),vec2(4.,2.),vec2(144.,56.));
 let bl=decal(vec2(p.x,s.y-p.y),vec2(4.,2.),vec2(144.,56.));
 let br=decal(s-p,vec2(4.,2.),vec2(144.,56.));
 let pressure=.78+.14*pulp;
 c=mix(c,tl.rgb,tl.a*pressure); c=mix(c,tr.rgb,tr.a*pressure);
 c=mix(c,bl.rgb,bl.a*pressure); c=mix(c,br.rgb,br.a*pressure);
 return vec4(c,aa(edge));
''',
'adv': '''// Cinema matte: a fixed brass name baseline, no ambient animation.
 let edge=max(rect(q,b,6.),abs(q.x)+abs(q.y)-b.x-b.y+12.);
 var c=mix(rgb(11.,12.,20.),rgb(25.,26.,36.),1.-i.uv.y);
 c=mix(c,rgb(106.,106.,121.),line(edge+1.,.4)*.65);
 let header=segment(p,vec2(42.,45.),vec2(min(s.x-70.,s.x*.53),45.),.45);
 c=mix(c,rgb(207.,177.,115.),header*.55);
 let diamond=aa(abs(p.x-41.)+abs(p.y-45.)-2.7);
 c=mix(c,rgb(236.,211.,162.),diamond*.85);
 c=mix(c,rgb(207.,177.,115.),max(segment(p,vec2(13.,13.),vec2(26.,13.),.7),segment(p,vec2(s.x-13.,s.y-24.),vec2(s.x-13.,s.y-13.),.7))*.6);
 return vec4(c,aa(edge)*.96);
''',
'rpg': '''// Enamel game window: stepped corners, three bevel bands and inset rivets.
 let edge=max(max(abs(q.x)-b.x,abs(q.y)-b.y),min(abs(q.x)-b.x+6.,abs(q.y)-b.y+6.));
 var c=mix(rgb(12.,23.,89.),rgb(42.,76.,159.),1.-i.uv.y);
 c=mix(c,rgb(244.,247.,255.),line(edge+1.4,1.3));
 c=mix(c,rgb(111.,158.,230.),line(edge+4.6,.55));
 c=mix(c,rgb(5.,12.,52.),line(edge+7.,1.));
 let top=segment(p,vec2(22.,11.),vec2(s.x-22.,11.),.45);
 c=mix(c,rgb(121.,174.,240.),top*.65);
 let studs=max(aa(max(abs(p.x-12.),abs(p.y-s.y+12.))-2.),aa(max(abs(p.x-s.x+12.),abs(p.y-12.))-2.));
 c=mix(c,rgb(225.,238.,255.),studs*.8);
 // The READY crystal belongs to the game's top-right bezel. It only
 // takes two pixel-grid positions while waiting for the next page.
 let bob=select(0.,2.*step(0.,sin(t*4.)),i.waiting>.5);
 let crystal=p-vec2(s.x-36.,13.+bob);
 let diamond=aa(abs(crystal.x)+abs(crystal.y)-4.5);
 c=mix(c,rgb(168.,205.,255.),diamond*.95);
 c=mix(c,rgb(244.,247.,255.),diamond*step(crystal.x,0.)*.6);
 return vec4(c,aa(edge));
''',
'vanilla': '''// Upright stationery: a vertical blue spine and level editorial rules.
 let edge=rect(q,b,select(0.,6.,p.x>s.x*.5));
 var c=mix(rgb(237.,243.,255.),rgb(255.,255.,255.),smoothstep(0.,.38,i.uv.x));
 c=mix(c,rgb(74.,125.,255.),aa(p.x-5.));
 c=mix(c,rgb(215.,226.,250.),segment(p,vec2(26.,s.y-12.),vec2(s.x-22.,s.y-12.),.4)*.8);
 let registration=max(segment(p,vec2(s.x-59.,s.y-12.),vec2(s.x-32.,s.y-12.),.65),segment(p,vec2(s.x-25.,s.y-12.),vec2(s.x-21.,s.y-12.),.65));
 c=mix(c,rgb(74.,125.,255.),registration*.65);
 return vec4(c,aa(edge));
''',
'bubble': '''// A soft heart sticker squashes and rocks; its ink and paper move together.
 let edge=rect(q,b,29.);
 var c=rgb(255.,255.,255.);
 c=mix(c,rgb(255.,232.,241.),line(edge+6.,1.9)*.75);
 c=mix(c,rgb(43.,43.,58.),line(edge+1.6,1.65));
 let bounce=pow(max(0.,sin(t*2.)),6.);
 let angle=.10*sin(t*2.); let ca=cos(angle); let sa=sin(angle);
 let v=p-vec2(s.x-38.,19.-2.8*bounce);
 let h=vec2(ca*v.x-sa*v.y,sa*v.x+ca*v.y)/vec2(1.+.10*bounce,1.-.12*bounce);
 let heart=min(min(length(h-vec2(-2.4,-1.8))-3.6,length(h-vec2(2.4,-1.8))-3.6),(abs(h.x)+abs(h.y-1.1)-5.1)*.707);
 c=mix(c,rgb(106.,36.,69.),aa(heart-1.1));
 c=mix(c,rgb(255.,151.,185.),aa(heart));
 c=mix(c,rgb(255.,246.,250.),segment(h,vec2(-3.,-3.),vec2(-1.8,-3.7),.55));
 let peel=segment(p,vec2(s.x-26.,24.),vec2(s.x-19.,18.),.7);
 c=mix(c,rgb(80.,169.,229.),peel*.75);
 let bridge=segment(p,vec2(25.,8.),vec2(25.,25.),1.1);
 c=mix(c,rgb(255.,122.,168.),bridge*.8);
 return vec4(c,aa(edge));
''',
'livechat': '''// Broadcast card: a shared cyan header rail joins avatar, name and on-air badge.
 let edge=rect(q,b,17.);
 var c=rgb(255.,255.,255.);
 c=mix(c,rgb(224.,243.,250.),line(edge+1.,.6)*.9);
 c=mix(c,rgb(238.,249.,252.),aa(p.y-37.));
 let rail=segment(p,vec2(20.,38.),vec2(s.x-20.,38.),.45);
 c=mix(c,rgb(35.,173.,229.),rail*.23);
 // Keep status below LIVE, clear of an unframed long name's ellipsis.
 let typing=1.-step(.999,i.reveal);
 let d0=disc(p-vec2(s.x-47.,35.-typing*1.6*max(0.,sin(t*4.))),1.5);
 let d1=disc(p-vec2(s.x-40.,35.-typing*1.6*max(0.,sin(t*4.-1.2))),1.5);
 let d2=disc(p-vec2(s.x-33.,35.-typing*1.6*max(0.,sin(t*4.-2.4))),1.5);
 c=mix(c,rgb(35.,151.,185.),max(d0,max(d1,d2))*.72);
 return vec4(c,aa(edge));
''',
'comic': '''// Inked panel: corner screen tone is trimmed by the panel and left clear of text.
 let edge=max(rect(q,b,3.),abs(q.x)+abs(q.y)-b.x-b.y+8.);
 var c=rgb(255.,255.,255.);
 let cell=fract(p/5.)*5.-2.5;
 let upper=clamp(1.-((s.x-p.x)+p.y*.7)/88.,0.,1.);
 let upper_margin=max(1.-smoothstep(23.,29.,p.y),smoothstep(s.x-25.,s.x-19.,p.x));
 let lower=clamp(1.-(p.x+(s.y-p.y)*.9)/68.,0.,1.);
 let lower_margin=max(1.-smoothstep(17.,23.,p.x),smoothstep(s.y-18.,s.y-12.,p.y));
 let dots=disc(cell,.65+upper*.85)*upper*upper_margin;
 let low_dots=disc(cell,.8+lower*.45)*lower*lower_margin;
 c=mix(c,rgb(17.,17.,17.),max(dots*.84,low_dots*.58));
 c=mix(c,rgb(0.,0.,0.),line(edge+1.8,1.8));
 return vec4(c,aa(edge));
''',
'terminal': '''// A four-state command spinner in the title bar, no travelling phosphor.
 let edge=max(rect(q,b,2.),abs(q.x)+abs(q.y)-b.x-b.y+5.);
 var c=rgb(9.,16.,11.);
 c=mix(c,rgb(21.,41.,27.),aa(p.y-24.));
 c=mix(c,rgb(46.,100.,60.),line(edge+1.,.55)*.9);
 c=mix(c,rgb(47.,105.,62.),line(p.y-24.,.45));
 let dot1=disc(p-vec2(15.,12.),3.); let dot2=ring(p-vec2(28.,12.),2.5,.6); let dot3=ring(p-vec2(41.,12.),2.5,.6);
 c=mix(c,rgb(75.,218.,111.),dot1*.75);
 c=mix(c,rgb(76.,136.,89.),max(dot2,dot3)*.9);
 let state=floor(fract(i.phase*8.)*4.);
 let v=p-vec2(62.,12.);
 var cursor=segment(v,vec2(-3.,-3.),vec2(3.,3.),.65);
 if(state<1.) { cursor=segment(v,vec2(0.,-4.),vec2(0.,4.),.65); }
 else if(state<2.) { cursor=segment(v,vec2(-3.,3.),vec2(3.,-3.),.65); }
 else if(state<3.) { cursor=segment(v,vec2(-4.,0.),vec2(4.,0.),.65); }
 if(i.waiting>.5) { cursor=line(max(abs(v.x),abs(v.y))-3.,.55); }
 c=mix(c,rgb(91.,228.,126.),cursor*.85);
 let notch=segment(p,vec2(s.x-111.,8.),vec2(s.x-111.,17.),.55);
 c=mix(c,rgb(51.,255.,102.),notch*(.5+.2*speaker));
 return vec4(c,aa(edge));
''',
'pixel': '''// Discrete LCD activity cells, exactly three solid ink tones, no breathing.
 let edge=max(max(abs(q.x)-b.x,abs(q.y)-b.y),min(abs(q.x)-b.x+6.,abs(q.y)-b.y+6.));
 var c=rgb(155.,188.,15.);
 c=mix(c,rgb(15.,56.,15.),line(edge+1.8,1.8));
 c=mix(c,rgb(48.,98.,48.),line(edge+5.5,.9));
 let edge_shade=aa(11.-p.y)*aa(p.y-7.);
 c=mix(c,rgb(139.,172.,15.),edge_shade*.55);
 let local=vec2(floor((s.x-p.x-17.)/8.),p.y-10.);
 let meter=aa(abs(local.y)-1.)*aa(abs(fract((s.x-p.x-17.)/8.)*8.-4.)-2.)*step(0.,local.x)*(1.-step(3.,local.x));
 let current=floor(fract(i.phase*4.)*3.);
 let on=1.-step(.1,abs(local.x-current));
 let ink=mix(rgb(139.,172.,15.),rgb(48.,98.,48.),on);
 c=mix(c,ink,meter);
 let bolt=max(aa(max(abs(p.x-11.),abs(p.y-s.y+11.))-2.),aa(max(abs(p.x-s.x+11.),abs(p.y-11.))-2.));
 c=mix(c,rgb(15.,56.,15.),bolt);
 return vec4(c,aa(edge));
''',
'sakura': '''// A branch flexes about its attachment; the paper and pink rim stay still.
 let edge=rect(q,b,23.);
 var c=mix(rgb(255.,244.,248.),rgb(253.,228.,238.),i.uv.y);
 c=mix(c,rgb(230.,150.,180.),line(edge+1.,.6)*.9);
 c=mix(c,rgb(255.,254.,251.),line(edge+5.,.55)*.9);
 let hinge=vec2(s.x-8.,29.); let v=p-hinge;
 let angle=.020*sin(t)+.006*sin(t*2.);
 let bent=hinge+vec2(cos(angle)*v.x-sin(angle)*v.y,sin(angle)*v.x+cos(angle)*v.y);
 let branch=decal(bent,vec2(s.x-134.,0.),vec2(128.,32.));
 c=mix(c,branch.rgb,branch.a);
 return vec4(c,aa(edge));
''',
'ink': '''// Rice-paper fibres and a dry-brush edge; the licensed bamboo remains untouched.
 let edge=rect(q,b,3.);
 var c=mix(rgb(254.,251.,244.),rgb(239.,233.,219.),length(q/s)*.7);
 let fibre=grain(vec2(p.x*.35,p.y*2.));
 c*=.988+.023*fibre;
 let dry=line(edge+1.2,.35)*(.42+.22*grain(p*.7));
 c=mix(c,rgb(122.,111.,90.),dry);
 let rule=segment(p,vec2(28.,s.y-11.),vec2(s.x-95.,s.y-11.),.35);
 c=mix(c,rgb(147.,134.,110.),rule*.25);
 let red=segment(p,vec2(26.,8.),vec2(26.,29.),.55);
 c=mix(c,rgb(176.,48.,42.),red*.58);
 return vec4(c,aa(edge));
''',
'echo_vanilla': '''// Echo proportions remain intact; optical ink weights and a quiet top registration.
 let edge=max(abs(q.x)-b.x,abs(q.y)-b.y);
 var c=rgb(255.,255.,255.);
 c=mix(c,rgb(198.,200.,204.),line(p.y-1.,.55));
 let rule=segment(p,vec2(18.,5.),vec2(96.,5.),.3);
 c=mix(c,rgb(171.,175.,181.),rule*.32);
 return vec4(c,aa(edge));
''',
'echo_bubble': '''// Echo's grey keyline and hard shadow retain their original footprint.
 let edge=rect(q,b,19.);
 var c=rgb(255.,255.,255.);
 c=mix(c,rgb(136.,136.,136.),line(edge+2.3,2.3));
 c=mix(c,rgb(237.,238.,240.),line(edge+6.5,.45)*.8);
 return vec4(c,aa(edge));
''',
}

def replace_section(text, heading, transform):
    pattern = rf'(\[{re.escape(heading)}\]\n)(.*?)(?=\n\[|\Z)'
    return re.sub(pattern, lambda m: m[1] + transform(m[2]), text, count=1, flags=re.S)

def put(section, key, value):
    return re.sub(rf'(?m)^{re.escape(key)} = .*$', f'{key} = {value}', section) if re.search(rf'(?m)^{re.escape(key)} = ', section) else section.rstrip()+f'\n{key} = {value}\n'

def drop(section, key):
    return re.sub(rf'(?m)^{re.escape(key)} = .*\n?', '', section)

def manifests():
    for name in NAMES:
        path = BUILTIN / name / 'skin.toml'
        text = path.read_text(encoding='utf-8')
        # Already generated entries are replaced, never accumulated.
        text = re.sub(r'\n\[shaders\.artwork\]\n.*?(?=\n\[|\Z)', '', text, flags=re.S)
        effects = '[{ shader = "artwork", target = "artwork" }]' if name in ANIMATED else '[]'
        text = replace_section(text, 'dialogue', lambda s: put(s, 'shader_effects', effects))
        if name == 'echo_glass':
            text = replace_section(text, 'dialogue.box', lambda s: re.sub(r'(?m)^frame = .*\n?', '', s))
        else:
            text = replace_section(text, 'dialogue.box', lambda s: put(s, 'frame', f'{{ image = "still.png", slice = {SLICES[name]}, layout_inset = false }}'))
        if name not in ['ink', 'livechat', 'echo_vanilla', 'echo_glass', 'echo_bubble']:
            text = re.sub(r'\n\[\[dialogue\.decor\]\]\n.*?(?=\n\[|\Z)', '', text, flags=re.S)
        elif name == 'livechat':
            # Avatar and LIVE are actual labelled UI, not part of the plate.
            text = re.sub(r'\n\[\[dialogue\.decor\]\]\n(?:(?!\n\[).)*?size = \[5, 5\].*?(?=\n\[|\Z)', '', text, flags=re.S)
        texture = f'textures = ["ornament.png"]\n' if name in ['parchment', 'sakura'] else ''
        if name in ANIMATED:
            text += f'\n[shaders.artwork]\nsource = "artwork.wgsl"\nperiod_s = 16\nintensity = 1\n{texture}'
        # Nameplates have a material of their own, joined to the plate by its engraving.
        tweaks = {
            'halo': dict(radius='999', border_color='"#D7EFFF"', border_width='0.8', gradient='{ angle = 90, stops = [[0, "#087FD1"], [1, "#249FDF"]] }', shadows='[{ color = "#0876d033", dy = 2, blur = 6 }]'),
            'night': dict(gradient='{ angle = 180, stops = [[0, "#193953"], [1, "#101D30"]] }', radius='3'),
            'parchment': dict(border_color='"#C9A46A"', border_width='1', gradient='{ angle = 180, stops = [[0, "#75462B"], [1, "#51301F"]] }'),
            'rpg': dict(border_color='"#EDF5FF"', border_width='2', radius='0'),
            'bubble': dict(text_color='"#6A2445"', gradient='{ angle = 180, stops = [[0, "#FF95B8"], [1, "#F36D9D"]] }', rotate='-2'),
            'comic': dict(gradient='{ angle = 180, stops = [[0, "#FFDE40"], [1, "#FFCA00"]] }', rotate='-3'),
            'pixel': dict(border_color='"#306230"', border_width='2'),
            'sakura': dict(border_color='"#F5B3CA"', border_width='0.8', color='"#FAD7E5"', gradient='{ angle = 180, stops = [[0, "#FFEAF2"], [1, "#FACBDE"]] }', radius='12'),
            'livechat': dict(color='"#00000000"', text_color='"#137C9A"', radius='0', padding='[0, 2]', border_width='0'),
        }
        for key, value in tweaks.get(name, {}).items():
            text = replace_section(text, 'dialogue.nameplate', lambda s, k=key, v=value: put(s, k, v))
        if name == 'halo':
            # Native radius 999 makes a true pill. The contour presets limit
            # their corner cuts to a fraction of the label's width/height.
            text = replace_section(text, 'dialogue.nameplate', lambda s: drop(s, 'contour'))
            text = replace_section(text, 'dialogue.box', lambda s: put(put(s, 'contour', '"rounded"'), 'radius', '30'))
            # A light capsule needs less elevation, not extra mechanical trim.
            text = re.sub(r'shadows = \[\n.*?\n\]', 'shadows = [{ color = "#2388C922", dy = 5, blur = 18 }]', text, count=1, flags=re.S)
            text = replace_section(text, 'history', lambda s: put(put(s, 'bubble_contour', '"rounded"'), 'bubble_radii', '[18, 18, 18, 18]'))
        elif name == 'vanilla':
            text = replace_section(text, 'dialogue.box', lambda s: put(s, 'contour', '"rounded"'))
            text = replace_section(text, 'history', lambda s: put(s, 'bubble_contour', '"rounded"'))
        elif name == 'livechat':
            for key in ['gradient', 'border_color', 'shadows']:
                text = replace_section(text, 'dialogue.nameplate', lambda s, k=key: drop(s, k))
            text = replace_section(text, 'history', lambda s: put(drop(s, 'name_plate'), 'name_color', '"#137C9A"'))
            text = text.replace('圆形头像、名字胶囊与白色消息气泡', '圆形头像、纯文字姓名与白色消息气泡').replace('round avatar, name pill and white message bubbles', 'round avatar, plain name and white message bubbles').replace('丸いアイコン、名前ピル、白い吹き出し', '丸いアイコン、文字の名前、白い吹き出し')
        # Remove comments describing deleted primitive nodes.
        text = re.sub(r'(?m)^# (?:Accent bar|Sparkle on|Curved four|Drifting petals|Inner rule|Title bar|Fusion Pixel is drawn|multiples of 12).*\n', '', text)
        history_plates = {
            'halo': '{ color = "name", gradient = { angle = 90, stops = [[0, "#087FD1"], [1, "#249FDF"]] }, text_color = "name_text", radius = 999, padding = [9, 2] }',
            'sakura': '{ color = "#FAD7E5", gradient = { angle = 180, stops = [[0, "#FFEAF2"], [1, "#FACBDE"]] }, text_color = "#7A3050", radius = 9, padding = [8, 1] }',
        }
        if name in history_plates:
            text = replace_section(text, 'history', lambda s: put(s, 'name_plate', history_plates[name]))
        save_text(path, text.rstrip()+'\n')
        folder = BUILTIN / name
        folder.mkdir(exist_ok=True)
        if name in BODIES:
            common = COMMON.replace('fn effect(', PAPER+'fn effect(') if name == 'parchment' else COMMON
            save_text(folder / 'artwork.wgsl', common+BODIES[name]+'}\n')
        # Bootstrap only; production GPU baker replaces this before delivery.
        if name != 'echo_glass' and not (folder / 'still.png').exists():
            Image.new('RGBA', (2, 2)).save(folder / 'still.png')

    # Transparent designs are typographic compositions, without a shader clock.
    for name in ['subtitle', 'echo_void']:
        path = BUILTIN / name / 'skin.toml'
        text = path.read_text(encoding='utf-8')
        if name == 'subtitle':
            text = replace_section(text, 'dialogue.text', lambda s: put(put(s, 'line_height', '1.38'), 'shadow', '{ color = "#000000A0", dy = 2, blur = 2 }'))
            text = replace_section(text, 'dialogue.nameplate', lambda s: put(put(s, 'text_color', '"#FFE1A3"'), 'stroke', '{ color = "#111115", width = 3.5 }'))
            text = replace_section(text, 'history', lambda s: put(s, 'name_color', '"#FFE1A3"'))
        else:
            text = replace_section(text, 'dialogue.nameplate', lambda s: put(s, 'text_color', '"#202124"'))
            text = replace_section(text, 'dialogue.text', lambda s: put(s, 'color', '"#202124"'))
            text = text.replace('text_color = "#888888"', 'text_color = "#73767C"')
        save_text(path, text)

def svg_masters():
    # Bookbinding reference: thin fillets with corner fleurons (Armagh,
    # Frames / Narrow Rolls; Huntington archival binding). Original drawing:
    # one continuous stem, attached asymmetric leaves and a thinning terminal.
    # The entire master is L-shaped; no shader cutout crosses a leaf.
    head = '<svg xmlns="http://www.w3.org/2000/svg" width="360" height="140" viewBox="0 0 360 140"><title>NanaType original CC0 bookbinding leaf fillet</title><desc>Original sprig drawing informed by historical bookbinding fillets and corner fleurons, not traced from a collection image.</desc><defs><g id="impression" stroke-linecap="round" stroke-linejoin="round">'

    def stem(points):
        commands = [f'M{points[0][0]} {points[0][1]}']
        for k in range(len(points)-1):
            prev = points[max(0,k-1)]
            here, nxt = points[k:k+2]
            after = points[min(k+2,len(points)-1)]
            c1 = [here[j]+(nxt[j]-prev[j])/6 for j in range(2)]
            c2 = [nxt[j]-(after[j]-here[j])/6 for j in range(2)]
            commands.append(f'C{c1[0]:.2f} {c1[1]:.2f} {c2[0]:.2f} {c2[1]:.2f} {nxt[0]} {nxt[1]}')
        return '<path d="'+' '.join(commands)+'" fill="none" stroke="currentColor" stroke-width="1.45"/>'

    def leaf(x,y,length,width,angle):
        # One pointed, asymmetric leaf with a shallow notch and a fine midrib.
        return (f'<g transform="translate({x} {y}) rotate({angle})">'
                f'<path d="M0 0C{length*.15:.2f} {width*.7:.2f} {length*.65:.2f} {width:.2f} {length} 0'
                f'C{length*.76:.2f} {-width*.10:.2f} {length*.58:.2f} {-width*.78:.2f} {length*.43:.2f} {-width*.45:.2f}'
                f'C{length*.29:.2f} {-width*.21:.2f} {length*.12:.2f} {-width*.55:.2f} 0 0Z" fill="currentColor" fill-opacity=".22" stroke="currentColor" stroke-width="1.05"/>'
                f'<path d="M1 0Q{length*.43:.2f} {width*.12:.2f} {length*.89:.2f} 0" fill="none" stroke="currentColor" stroke-width=".62"/></g>')

    top = [(12,18),(30,12),(50,18),(70,25),(92,19),(112,12),(135,18),(156,24),(180,18),(204,12),(229,18),(256,23),(284,18),(316,16),(348,18)]
    side = [(12,18),(19,38),(12,56),(17,77),(22,95),(16,114),(12,132)]
    art = stem(top)+stem(side)
    top_leaves = [(12,18,25,7,29),(30,12,23,6,-18),(30,12,24,7,24),(50,18,22,6,28),(70,25,22,6,-18),(92,19,20,5.8,-32),(112,12,18,5,17),(135,18,18,5,35),(156,24,18,4.8,-20),(180,18,17,4.6,-30),(204,12,16,4.3,28),(229,18,15,4,36),(256,23,13,3.7,-30),(284,18,12,3.3,-25),(316,16,9,2.6,32)]
    side_leaves = [(19,38,24,6,-30),(12,56,23,6,-26),(17,77,22,5.5,-25),(22,95,18,5,-22),(16,114,14,4,-25),(12,132,9,2.8,-45)]
    art += ''.join(leaf(*spec) for spec in top_leaves+side_leaves)
    # A minute displaced light impression gives pressed fibres, not metallic
    # gloss. There is no blur filter, external image, text or runtime SVG.
    save_text(BUILTIN/'parchment/ornament.svg', head+art+'</g></defs><use href="#impression" color="#FFF2D3" transform="translate(.35 1.3)" opacity=".60"/><use href="#impression" color="#8F683B" opacity=".88"/></svg>\n')
    art = '<svg xmlns="http://www.w3.org/2000/svg" width="384" height="132" viewBox="0 0 384 132"><title>NanaType original CC0 cherry branch</title><defs><radialGradient id="petal"><stop stop-color="#FFF9F1"/><stop offset=".72" stop-color="#FAD3E0"/><stop offset="1" stop-color="#EBA1BF"/></radialGradient></defs>'
    art += '<path d="M379 122C345 89 318 76 284 69C248 62 224 55 193 35M306 77C316 53 335 39 353 24M250 60C229 79 209 91 179 98M211 45C182 38 151 39 128 47" fill="none" stroke="#AC6C7D" stroke-width="3.5" stroke-linecap="round"/>'
    art += '<path d="M375 121C342 91 318 82 292 75M306 77L332 45M250 60L223 78" fill="none" stroke="#D69BAB" stroke-width="1.2"/>'
    for x,y,r,a in [(283,66,24,-13),(334,40,17,15),(218,77,19,22),(187,34,15,-10),(143,44,12,17)]:
        art += f'<g transform="translate({x} {y}) rotate({a})">'
        for n in range(5):
            art += f'<path transform="rotate({n*72})" d="M0 2C{-r} -4 {-r*.85} {-r*.8} {-r*.30} {-r}L0 {-r*.79}L{r*.30} {-r}C{r*.85} {-r*.8} {r} -4 0 2Z" fill="url(#petal)" stroke="#D997B1" stroke-width=".65"/>'
        for dx,dy in [(-5,-5),(4,-6),(0,6),(7,2),(-6,2)]:
            art += f'<path d="M0 0L{dx} {dy}" stroke="#BF7999" stroke-width=".7"/><circle cx="{dx}" cy="{dy}" r="1" fill="#D0A16B"/>'
        art += '<circle r="2.2" fill="#E4B182"/></g>'
    art += '<path d="M182 98C173 82 155 85 148 99C161 107 175 108 182 98ZM352 25C349 9 334 9 329 15C332 28 340 31 352 25Z" fill="#98A98A" stroke="#768F75" stroke-width=".8"/>'
    save_text(BUILTIN/'sakura/ornament.svg', art+'</svg>\n')

if __name__ == '__main__':
    manifests()
    svg_masters()
    print('Authored seven object animations and eleven static designs; static WGSL is bake-only.')
