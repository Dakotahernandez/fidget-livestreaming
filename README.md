[![Review Assignment Due Date](https://classroom.github.com/assets/deadline-readme-button-22041afd0340ce965d47ae6ef1cefeee28c7c493a6346c4f15d667ab976d596c.svg)](https://classroom.github.com/a/mun0VlAh)
Goal: Apply the knowledge you've learned in new ways.

# Project description
This is an open-ended project. Students can extend their BearTV project or do something new from the ground up. Project ideas must be approved by Dr. Freeman.

## Current project state

Our project (Fidget: one-to-many live streaming with CDN-style delivery) is currently in an implementation-complete state for the core app path and partially complete for CDN hardening.

Completed:
- Dockerized multi-service local stack (frontend, backend, database, media server, edge, router)
- Backend auth + stream APIs in Rust/Axum
- WebTransport (QUIC) ingest path: browser -> backend QUIC -> ffmpeg fan-out -> RTMP/HLS pipeline (source / 1080p / 720p / 360p)
- WHIP browser ingest path: browser -> SRS WHIP -> `rtc_to_rtmp` bridge -> backend `on_publish` -> supervised ffmpeg rendition ladder (`backend/src/transcode.rs`) producing 720p / 360p
- Build-time `VITE_DISABLE_WEBTRANSPORT=1` switch forces WHIP-only ingest for CDN deployments that can't carry UDP/4443
- Stream-key validation in SRS `on_publish` / `on_publish_done` callbacks (with role check)
- WHEP playback (≤1.5 s first-frame timeout) with HLS fallback, multi-rendition quality selector (Source / 720p / 360p)
- Resilient publish lifecycle: 3 s debounce on SRS `on_publish_done` plus transient-aware ICE handling on the publisher so brief WebRTC hiccups don't end the broadcast
- Frontend pages for auth, dashboard, watch, and admin
- Router with GeoIP-aware nearest-edge selection + CloudFront / local fallbacks
- Edge nginx serving `/hls/<key>/...` from a read-only shared volume

In progress / remaining:
- Multi-region deployment validation (CloudFront + multiple edge nginx instances)
- Low-latency HLS / hardware-accelerated encoders for tighter glass-to-glass latency
- Automated load testing beyond `scripts/simulate_viewers.sh`

Deployment target:
- **AWS EC2** instance hosts the full Docker Compose stack (backend, media-server, frontend, db, edge, router)
- **AWS S3** bucket serves as the HLS origin (`/hls/<key>/index.m3u8` + `.ts` segments)
- **AWS CloudFront** fronts the S3 origin for global, cached HLS delivery; the router still selects CloudFront via `CLOUDFRONT_DOMAIN` / `EDGE_SERVERS`

Detailed technical status and architecture notes are maintained in `TEAMREADME.md` and `docs/`.

You must give a **formal presentation** of your project in place of a final exam. Each group will have ~12 minutes to present their work. Each member of the group must speak. You should have slides. Your presentation must include a demo of your project, although it may invlude a pre-recorded screen capture. In your presentation, you should introduce the problem that you addressed, how you addressed it, technical challenges you faced, what you learned, and next steps (if you were to continue developing it).

You may use AI LLM tools to assist with the development of your project, including code assistant tools like GitHub Copilot. If you do use any AI tools, you must describe your use during your presentation.

Unless you get specific approval otherwise, your project **must** include some component deployed on a cloud hosting service. You can use AWS, GCP, Azure, etc. These services have free tiers, and you might consider looking into tiers specifically for students.

**Graudate students enrolled in CSI-5321:** You have additional requirements. See the bottom of the README.

## Milestones
- You must present your project idea to Dr. Freeman within the first week to get it approved
- You must meet with Dr. Freeman within the first 3 weeks to give a status update and discuss roadblocks
- See the course schedule spreadhseet for specific dates

## Project Ideas
- Simulate UDP packet loss and packet corruption in BearTV in a non-deterministic way (i.e., don't just drop every Nth packet). Then, extend the application protocol to be able to detect and handle this packet loss.
- Extend the BearTV protocol to support streaming images (or video!) alongside the CC data, and visually display them on the client. This should be done in such a way that it is safely deliver*able* over *any* implementation of IPv4. The images don't have to be relevant to the caption data--you can get them randomly on the server from some image source.
- Do something hands on with a video streaming protocol such as MoQ, DASH, or HLS.
- Implement QUIC
- Develop a new congestion control algorithm and evaluate it compared to existing algorithms in a realistic setting
- Make significant contributions to a relevant open-source repository (e.g., moq-rs)
- Implement a VPN
- Implement a DNS
- Do something with route optimization
- Implement an HTTP protocol and have a simple website demo

--> These are just examples. I hope that you'll come up with a better idea to suit your own interests!

## Libraries

Depending on the project, there may be helpful libraries you find to help you out. However, there may also be libraries that do all the interesting work for you. Depending on the project, you'll need to determine what should be fair game. For example, if your project is to implement HTTP, then you shouldn't leverage an HTTP library that does it for you.

If you're unsure if a library is okay to use, just ask me.

## Languages

The core of your project should, ideally, be written in Rust. Depending on the project idea, however, I'm open to allowing the use of other languages if there's a good reason for it. For me to approve such a request, the use of a different language should enable greater learning opportunities for your group.

# Submission

## Questions
- What is your project?

Fidget is a one-to-many live streaming platform with CDN-style delivery. Browsers ingest video over WebTransport (QUIC) or WHIP (WebRTC), the Rust/Axum backend transcodes into an HLS rendition ladder, and a GeoIP-aware router steers viewers to the nearest edge (local nginx or CloudFront) for low-latency playback.

### Dakota

**What novel work did you do?**
Scaffolded the project and built most of the streaming pipeline: a Rust QUIC/WebTransport ingest server with JWT auth and ffmpeg fan-out, a WHIP fallback for Firefox/Safari, and a WHEP viewer with HLS fallback that cut local latency from ~2.7 s to near-instant. Migrated the media server from nginx-rtmp to SRS for LL-HLS, wrote the WHIP->RTMP rendition ladder bridge.Finally the role-application/admin flow.

**What did you learn?**
A lot about the realities of live video on the web NSthe level of fine tuned it is to work so flawlessly on services i use everyd day, WebTransport vs. WHIP/WHEP tradeoffs, how libx264 cares about even dimensions (the 761×877 canvas bug), how to keep ffmpeg from stalling by draining stderr. I also learned how much infrastructure work sits between "it works on localhost" and "it works deployed on EC2."

**What was challenging?**
Crossbrowser ingest was the hardest part — WebTransport only really works on Chromium, so I had to design the system to gracefully fall back to WHIP and still feed the same HLS ladder. Latency tuning was also painful: chasing down a ~2.7 s delay led me through MediaRecorder timeslice tweaks, skipping libx264 when the browser already sends H.264, swapping to SRS for LL-HLS, and finally moving the viewer to WHEP.

**What AI tools did you use, and what did you use them for? What were their benefits and drawbacks?**
I used GitHub Copilot (Claude/GPT models in the chat side panel) heavily for Dockerfiles, nginx configs, and explaining unfamiliar APIs . The benefit was speed: it saved hours on config syntax and let me prototype the QUIC ingest much faster. The drawback was that it confidently produced wrong code for anything  i was unfamiliar with so I had to read docs anywaysto actually get things working.

**What would you do differently next time?**
Pick a tried and tested architecture that's known to scale before starting, instead of swapping core pieces (nginx-rtmp -> SRS) mid-project. I'd also set up AWS so everyone on the team can touch the servers and services from day one rather than having it bottlenecked through one person, shared access, shared deploy scripts, and the EC2/CloudFront environment.

### Josh
- What novel work did you do?
- What did you learn?
- What was challenging?
- What AI tools did you use, and what did you use them for? What were their benefits and drawbacks?
- What would you do differently next time?

### Chandler
- What novel work did you do?
- What did you learn?
- What was challenging?
- What AI tools did you use, and what did you use them for? What were their benefits and drawbacks?
- What would you do differently next time?

### Ben
- What novel work did you do?
- 	Streamer keys and FFMPeg qualities. Google Slides. Doccumentation.
- What did you learn?
- 	I learned that FFMPeg is extremly useful for parsing video and audio files. It made it very easy to convert our data to whatever format we needed it for. I also learned that it can be vvery hard to debug the applicatipn layer tools that we used/trying to use. We ened up changning tools multiple times because we were not able to find the bugs in what we had. We also tried to have fallbacks for most of our tools incase the main branch failed.
- What was challenging?
- 	The hardest part was figuring out what tools to use. We did not spend enough time reseaching so we refactored our project many times which eventaully required us to restructure it. 
- What AI tools did you use, and what did you use them for? What were their benefits and drawbacks?
- 	I used claude to generate diagrams and to give me commands to test my code.
- What would you do differently next time?
- 	More preperation, try to figure out how all aspects of the architecture work before developing. The big picture was very overwhelming at the beginning so just started developing small fragments but they did not come together well.

## What to submit
- Push your working code to the main branch of your team's GitHub Repository before the deadline
- Edit the README to answer the above questions
- On Teams, *each* member of the group must individually upload answers to these questions:
	- What did you (as an individual) contribute to this project?
	- What did the other members of your team contribute?
	- Do you have any concerns about your own performance or that of your team members? Any comments will remain confidential, and Dr. Freeman will try to address them in a way that preserves anonymity.
	- What feedback do you have about this course?

## Grading

Grading will be based on...
- The technical merit of the group's project
- The contribution of each individual group member
- Evidence of consistent work, as revealed during milestone meetings
- The quality of the final presentation

# 5321 Extra requirements

(For graudate students enrolled in CSI-5321)

Your project **must** have a research component to it. That is, you must set out to address an open research question using some networking concepts you've learned in this course. Subject to my approval, this project can be an extension of a project you're currently working on with your advisor.

Along with your code repository and presentation, you must submit a 4+ page paper (plus biblography), in double-column paper using the [ACM LaTeX template](https://www.acm.org/publications/proceedings-template) under the `sigconf` document class. This paper should include a brief introduction, related work, a description of your approach, experimental results, and a conclusion. Your goal should be to have a paper of high enough quality that it can be easily extended for a full submission to a research conference.

All other instructions for the project apply, including the above submission questions.

## 5321 Research ideas

- Investigate the efficacy of congestion control algorithms for QUIC. Design your own improvements for some application, and evaluate it.
- Propose improvements to, or novel applications of, the Media Over QUIC protocol
- Limitations of QUIC IP switching
- Quality adaptation for lossy data streaming of something other than traditional video
- Distributed databases, federation, or blockchain
- Protocol security flaws
- IoT, WiFi 8, or 6G
- CDN performance (e.g., content steering)
- Live video streaming with distributed super-resolution between server/client for higher quality and lower latency
- Mesh networking
- Machine learning for routing protocols

--> These are just examples. I hope that you'll come up with a better idea to suit your own interests!
