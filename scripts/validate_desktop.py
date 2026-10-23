#!/usr/bin/env python3
"""Optional real-time X11/Emerald soak using an isolated copy of a cartridge save.

Requires a dedicated Xvfb display and libX11; do not drive a live desktop.
ALSA mode uses a null PCM device, never speakers.
This is automated movement/transport validation, not a human listening test.
"""
import argparse
import ctypes as C
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

class Key(C.Structure):
 _fields_=[('type',C.c_int),('serial',C.c_ulong),('send_event',C.c_int),('display',C.c_void_p),('window',C.c_ulong),('root',C.c_ulong),('subwindow',C.c_ulong),('time',C.c_ulong),('x',C.c_int),('y',C.c_int),('x_root',C.c_int),('y_root',C.c_int),('state',C.c_uint),('keycode',C.c_uint),('same_screen',C.c_int)]
class Event(C.Union):_fields_=[('key',Key),('padding',C.c_long*24)]
class Image(C.Structure):
 _fields_=[('width',C.c_int),('height',C.c_int),('xoffset',C.c_int),('format',C.c_int),('data',C.c_void_p),('byte_order',C.c_int),('bitmap_unit',C.c_int),('bitmap_bit_order',C.c_int),('bitmap_pad',C.c_int),('depth',C.c_int),('stride',C.c_int),('bpp',C.c_int),('red_mask',C.c_ulong),('green_mask',C.c_ulong),('blue_mask',C.c_ulong)]

class X11:
    def __init__(self):
        self.x = C.CDLL("libX11.so.6")
        signatures = {
            "XOpenDisplay": ([C.c_char_p], C.c_void_p),
            "XDefaultRootWindow": ([C.c_void_p], C.c_ulong),
            "XQueryTree": ([C.c_void_p,C.c_ulong,C.POINTER(C.c_ulong),C.POINTER(C.c_ulong),C.POINTER(C.POINTER(C.c_ulong)),C.POINTER(C.c_uint)],C.c_int),
            "XFetchName": ([C.c_void_p,C.c_ulong,C.POINTER(C.c_char_p)],C.c_int),
            "XFree": ([C.c_void_p],C.c_int),
            "XKeysymToKeycode": ([C.c_void_p,C.c_ulong],C.c_ubyte),
            "XSendEvent": ([C.c_void_p,C.c_ulong,C.c_int,C.c_long,C.c_void_p],C.c_int),
            "XFlush": ([C.c_void_p],C.c_int),
            "XGetImage": ([C.c_void_p,C.c_ulong,C.c_int,C.c_int,C.c_uint,C.c_uint,C.c_ulong,C.c_int],C.c_void_p),
            "XDestroyImage": ([C.c_void_p],C.c_int),
            "XCloseDisplay": ([C.c_void_p],C.c_int),
            "XInternAtom": ([C.c_void_p,C.c_char_p,C.c_int],C.c_ulong),
            "XGetWindowProperty": ([C.c_void_p,C.c_ulong,C.c_ulong,C.c_long,C.c_long,C.c_int,C.c_ulong,C.POINTER(C.c_ulong),C.POINTER(C.c_int),C.POINTER(C.c_ulong),C.POINTER(C.c_ulong),C.POINTER(C.c_void_p)],C.c_int),
        }
        for name,(arguments,result) in signatures.items():
            function=getattr(self.x,name);function.argtypes=arguments;function.restype=result
        self.display=self.x.XOpenDisplay(None)
        if not self.display: raise RuntimeError("no X11 display; use xvfb-run -a")
        self.root=self.x.XDefaultRootWindow(self.display)
        self.pid_atom=self.x.XInternAtom(self.display,b"_NET_WM_PID",False)
        self.window=None

    def find_window(self,pid):
        root=C.c_ulong();parent=C.c_ulong();children=C.POINTER(C.c_ulong)();count=C.c_uint()
        self.x.XQueryTree(self.display,self.root,C.byref(root),C.byref(parent),C.byref(children),C.byref(count))
        try:
            for index in range(count.value):
                actual=C.c_ulong();fmt=C.c_int();items=C.c_ulong();remaining=C.c_ulong();data=C.c_void_p()
                self.x.XGetWindowProperty(self.display,children[index],self.pid_atom,0,1,False,6,
                                         C.byref(actual),C.byref(fmt),C.byref(items),C.byref(remaining),C.byref(data))
                if data:
                    found=fmt.value==32 and items.value==1 and C.cast(data,C.POINTER(C.c_ulong))[0]==pid
                    self.x.XFree(data)
                    if found: self.window=children[index];return True
        finally:
            if children:self.x.XFree(children)
        return False

    def key(self,symbol,pressed):
        event=Event();event.key.type=2 if pressed else 3;event.key.display=self.display
        event.key.window=self.window;event.key.root=self.root;event.key.same_screen=1
        event.key.keycode=self.x.XKeysymToKeycode(self.display,symbol)
        event.key.time=int(time.monotonic()*1000)&0xffffffff
        if not self.x.XSendEvent(self.display,self.window,False,1 if pressed else 2,C.byref(event)):
            raise RuntimeError("key event delivery failed")
        self.x.XFlush(self.display)

    def tap(self,symbol):
        self.key(symbol,True);time.sleep(.15);self.key(symbol,False);time.sleep(.01)

    def title(self):
        name=C.c_char_p();self.x.XFetchName(self.display,self.window,C.byref(name))
        value=name.value or b""
        if name:self.x.XFree(name)
        return value

    def snapshot(self,scale=1):
        pointer=self.x.XGetImage(self.display,self.window,0,0,240*scale,160*scale,0xffffffffffffffff,2)
        if not pointer:raise RuntimeError("X11 image unavailable")
        try:
            image=C.cast(pointer,C.POINTER(Image)).contents
            if image.bpp!=32 or (image.red_mask,image.green_mask,image.blue_mask)!=(0xff0000,0xff00,0xff):
                raise RuntimeError("capture harness requires a 32-bpp RGB TrueColor X server")
            source=C.string_at(image.data,image.stride*160*scale);output=bytearray()
            for y in range(160):
                for x in range(240):
                    offset=y*scale*image.stride+x*scale*4
                    pixel=int.from_bytes(source[offset:offset+4],"little" if image.byte_order==0 else "big")
                    output.extend([(pixel>>16)&255,(pixel>>8)&255,pixel&255])
            return bytes(output)
        finally:self.x.XDestroyImage(pointer)

    def close(self):self.x.XCloseDisplay(self.display)


def artifact_executable(output):
    """Read the exact target path from Cargo, including custom target builds."""
    paths=set()
    for line in output.splitlines():
        if not line.startswith("{"):continue
        try:message=json.loads(line)
        except json.JSONDecodeError:continue
        if (message.get("reason")=="compiler-artifact" and
                message.get("target",{}).get("name")=="gba-desktop" and
                "bin" in message.get("target",{}).get("kind",[]) and message.get("executable")):
            paths.add(Path(message["executable"]))
    if len(paths)!=1:raise ValueError("Cargo did not report exactly one desktop executable")
    return paths.pop()


def validate(args):
    def notice_border(image):
        return image[(8*240+79)*3:(8*240+79)*3+3]

    output=args.output.resolve();save_source=args.save.resolve()
    original=save_source.read_bytes()
    if len(original)!=131072:raise ValueError("requires an existing 128 KiB Emerald save")
    output.mkdir(parents=True,exist_ok=False)
    save=output/"isolated.sav";save.write_bytes(original)
    rom_copy=output/"game.gba";shutil.copyfile(args.rom,rom_copy)
    build=subprocess.check_output(["sh",str(ROOT/"scripts/cargo.sh"),"build","--release","-p","gba-desktop","--message-format=json"],cwd=ROOT,text=True)
    binary=artifact_executable(build)
    env=dict(os.environ)
    xdg_data=output/"xdg-data";xdg_config=output/"xdg-config"
    env["XDG_DATA_HOME"]=str(xdg_data);env["XDG_CONFIG_HOME"]=str(xdg_config)
    identity=hashlib.sha256(rom_copy.read_bytes()).hexdigest()
    state_path=xdg_data/"rgba"/"states"/f"{identity}.state"
    settings_path=xdg_config/"rgba"/"settings"/f"{identity}.settings"
    if args.audio=="alsa":
        config=output/"alsa-null.conf";config.write_text("pcm.!default { type null }\n")
        env["ALSA_CONFIG_PATH"]=str(config)
    log_path=output/"desktop.log"
    report=dict(target_frames=args.frames,backend=args.audio,scale=args.scale,host=platform.platform(),
                isolated_rom_sha256=hashlib.sha256(rom_copy.read_bytes()).hexdigest(),
                save_sha256=hashlib.sha256(original).hexdigest(),max_rss_kib=0,
                binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
    start=time.monotonic();process=None;exit_process=None;display=None
    try:
        with log_path.open("x") as log:
            process=subprocess.Popen([str(binary),"--bios",str(args.bios.resolve()),"--rom",str(rom_copy.resolve()),
                                      "--save",str(save),"--scale",str(args.scale),"--audio",args.audio,"--frames",str(args.frames)],
                                     env=env,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT)
            display=X11()
            for _ in range(100):
                if display.find_window(process.pid):break
                time.sleep(.1)
            if not display.window:raise RuntimeError("child window not found by _NET_WM_PID")
            events=[(34,0xff0d),(41,0xff0d),(48,0xff0d),(56,0x7a)]
            index=0;paused=False;held=None;last_minute=-1;last_flush=-1
            menu_tested=False;bindings_tested=False;state_hotkeys_tested=False
            while process.poll() is None:
                elapsed=time.monotonic()-start
                if elapsed>args.frames*280896/16777216*1.5+90:raise TimeoutError("desktop run exceeded wall-time budget")
                if index<len(events) and elapsed>=events[index][0]:display.tap(events[index][1]);index+=1
                if not menu_tested and elapsed>=1:
                    display.tap(0xff1b);time.sleep(.2)
                    if b"OPTIONS" not in display.title():raise AssertionError("Escape did not open options")
                    display.tap(0xff0d);time.sleep(.2)
                    if not state_path.is_file():raise AssertionError("Save State menu action failed")
                    display.tap(0xff54);display.tap(0xff0d);time.sleep(.2)
                    if b"OPTIONS" not in display.title():raise AssertionError("Load State left options menu")
                    display.tap(0xff1b);time.sleep(.2)
                    if b"OPTIONS" in display.title():raise AssertionError("Escape did not resume gameplay")
                    display.tap(0xff1b)
                    for _ in range(4):display.tap(0xff54)
                    display.tap(0xff0d)
                    speed_settings=[]
                    for speed_index, value in enumerate(("2", "4", "8", "16", "1")):
                        display.tap(0xff54);display.tap(0xff0d);time.sleep(.15)
                        label=f"[{value}X]".encode()
                        if label not in display.title():raise AssertionError(f"speed title missing {label!r}")
                        expected=f"gba-speed-v1\n{value}\n"
                        if expected not in settings_path.read_text():raise AssertionError(f"speed setting {value} was not persisted")
                        speed_settings.append(value)
                        if speed_index < 4:display.tap(0xff0d)
                    display.tap(0xff1b);time.sleep(.2)
                    if b"OPTIONS" in display.title():raise AssertionError("speed test did not resume gameplay")
                    menu_tested=True;report["options_save_load_resume_tested"]=True
                    report["speed_values_persisted"]=speed_settings
                if not bindings_tested and elapsed>=3:
                    display.tap(0xff1b)
                    for _ in range(2):display.tap(0xff54)
                    display.tap(0xff0d)
                    for _ in range(12):display.tap(0xff54)
                    display.tap(0xff0d);display.tap(0xffc3)
                    settings=settings_path
                    saved_before_duplicate=settings.read_text()
                    display.tap(0x6d)
                    if settings.read_text()!=saved_before_duplicate:raise AssertionError("duplicate keybinding changed settings")
                    if "OPTIONS MENU=0000006d,00000000" not in settings.read_text():raise AssertionError("keybinding was not persisted")
                    display.tap(0x78);display.tap(0x78);time.sleep(.2)
                    if b"OPTIONS" in display.title():raise AssertionError("custom menu binding did not resume gameplay")
                    bindings_tested=True;report["menu_keybinding_persisted"]=True
                if not state_hotkeys_tested and elapsed>=7:
                    display.tap(0xffc3);time.sleep(.05)
                    if notice_border(display.snapshot(args.scale))!=bytes([112,224,144]):raise AssertionError("save-state success notice missing")
                    display.tap(0xffc4);time.sleep(.05)
                    if notice_border(display.snapshot(args.scale))!=bytes([112,224,144]):raise AssertionError("load-state success notice missing")
                    if not state_path.is_file():raise AssertionError("state hotkeys failed")
                    state_path.write_bytes(b"corrupt state")
                    display.tap(0xffc4);time.sleep(.05)
                    if notice_border(display.snapshot(args.scale))!=bytes([255,112,112]):raise AssertionError("load-state failure notice missing")
                    state_hotkeys_tested=True;report["state_hotkeys_tested"]=True
                    report["state_save_load_notices_tested"]=True
                if elapsed>=74 and not paused:
                    display.tap(0x20);time.sleep(.3)
                    if b"PAUSED" not in display.title():raise AssertionError("pause title missing")
                    image=display.snapshot(args.scale)
                    if image[(68*240+92)*3:(68*240+92)*3+3]!=bytes([255]*3):raise AssertionError("pause border missing")
                    if image[(70*240+100)*3:(70*240+100)*3+3]!=bytes([16,24,32]):raise AssertionError("pause panel missing")
                    time.sleep(1)
                    if display.snapshot(args.scale)!=image:raise AssertionError("paused display changed")
                    (output/"paused-window.ppm").write_bytes(b"P6\n240 160\n255\n"+image)
                    before=set((ROOT/"artifacts").glob(f"screenshot-*-{process.pid}.ppm"))
                    display.tap(0xffc9);time.sleep(.3)
                    captures=set((ROOT/"artifacts").glob(f"screenshot-*-{process.pid}.ppm"))-before
                    if len(captures)!=1:raise AssertionError("F12 screenshot missing")
                    shutil.copyfile(captures.pop(),output/"continued-game.ppm")
                    display.tap(0x20);paused=True;report["pause_overlay_and_frozen_display"]=True
                if elapsed>=85:
                    position=(elapsed-85)%30
                    desired=0xff51 if position<2 else 0xff53 if position<4 else None
                    if desired!=held:
                        if held:display.key(held,False)
                        if desired:display.key(desired,True)
                        held=desired
                    flush=int((elapsed-85)//300)
                    if flush!=last_flush:display.tap(0xffc2);last_flush=flush
                minute=int(elapsed//60)
                if minute!=last_minute:
                    last_minute=minute
                    status=Path(f"/proc/{process.pid}/status").read_text()
                    match=re.search(r"VmRSS:\s+(\d+)",status)
                    if match:report["max_rss_kib"]=max(report["max_rss_kib"],int(match[1]))
                    report.update(status="running",elapsed_wall_seconds=round(elapsed,3))
                    (output/"progress.json").write_text(json.dumps(report,indent=2)+"\n")
                time.sleep(.03)
            if process.wait()!=0:raise RuntimeError("desktop exited unsuccessfully; inspect desktop.log")
        text=log_path.read_text()
        match=re.search(r"Completed frames=(\d+) clocks=(\d+) PCM=(\d+) core_dropped=(\d+)",text)
        if not match:raise ValueError("completion metrics missing")
        report.update(dict(zip(["completed_frames","master_clocks","pcm_frames","core_dropped"],map(int,match.groups()))))
        drop=re.search(r"Audio host dropped frames: (\d+)",text)
        if args.audio!="off" and drop is None:raise ValueError("audio player did not open")
        report.update(host_dropped=int(drop[1]) if drop else None,elapsed_wall_seconds=round(time.monotonic()-start,3),
                      emulated_seconds=report["master_clocks"]/16777216,save_unchanged=save.read_bytes()==original,status="passed")
        if report["core_dropped"] or report["host_dropped"] not in (None,0):
            raise AssertionError("audio queue dropped frames")
        if (report["completed_frames"]!=args.frames or not paused or not report["save_unchanged"]
                or not menu_tested or not bindings_tested or not state_hotkeys_tested):
            raise AssertionError("frame/pause/save/options invariant failed")
        exit_log_path=output/"exit-test.log"
        with exit_log_path.open("x") as exit_log:
            exit_process=subprocess.Popen([str(binary),"--bios",str(args.bios.resolve()),"--rom",str(rom_copy.resolve()),
                                           "--save",str(save),"--scale",str(args.scale),"--audio","off","--frames",str(args.frames)],
                                          env=env,cwd=ROOT,stdout=exit_log,stderr=subprocess.STDOUT)
            display.window=None
            for _ in range(100):
                if display.find_window(exit_process.pid):break
                time.sleep(.1)
            if not display.window:raise RuntimeError("exit-test window did not map")
            time.sleep(.1)
            if b"[1X]" not in display.title():raise AssertionError("per-game 1X speed setting did not reload")
            display.tap(0x6d)
            if b"OPTIONS" not in display.title():raise AssertionError("saved menu binding did not reload")
            for _ in range(6):display.tap(0xff54)
            display.key(0xff0d,True);display.key(0xff0d,False)
            try:exit_status=exit_process.wait(timeout=10)
            except subprocess.TimeoutExpired as error:raise AssertionError("explicit Exit did not close the window") from error
            if exit_status!=0:raise RuntimeError("explicit Exit process failed; inspect exit-test.log")
        exit_text=exit_log_path.read_text()
        exit_match=re.search(r"Completed frames=(\d+)",exit_text)
        if not exit_match or int(exit_match[1])>=args.frames:raise AssertionError("explicit Exit did not stop the bounded run")
        report.update(settings_reloaded=True,explicit_exit_tested=True,exit_frames=int(exit_match[1]),
                      exit_save_unchanged=save.read_bytes()==original)
        if not report["exit_save_unchanged"]:raise AssertionError("explicit Exit changed the isolated save")
    except BaseException as error:
        report.update(status="failed",error=repr(error))
        if process and process.poll() is None:process.kill();process.wait()
        if exit_process and exit_process.poll() is None:exit_process.kill();exit_process.wait()
        raise
    finally:
        if display:display.close()
        if rom_copy.exists():rom_copy.unlink()
        (output/"report.json").write_text(json.dumps(report,indent=2)+"\n")
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bios",type=Path,default=ROOT/"bins/gba_bios.bin")
    parser.add_argument("--rom",type=Path,default=ROOT/"bins/Pokemon - Emerald Version (USA, Europe).gba")
    parser.add_argument("--save",type=Path,required=True)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--frames",type=int,default=115200)
    parser.add_argument("--scale",type=int,choices=range(1,7),default=1)
    parser.add_argument("--audio",choices=["alsa","pulse","pipewire","off"],default="alsa")
    args=parser.parse_args()
    if not 6000<=args.frames<=200000:parser.error("frames must be 6000..200000")
    print(json.dumps(validate(args),indent=2))


if __name__=="__main__":main()
