# I finally let agents drive

<p align="center"><img src="img/memory-cover.png" alt="Buzz shows Claude the whole board: see this? All your responsibility now" width="700"></p>

For more than a year now the number of agent sessions kept growing up until the point
it was hard for me to understand what all the different sessions were supposed to do.

I found myself doing the same things over and over again and mostly copy & pasting context
from different tools into a >10 times split terminal window. Adding colors or session names
only helped slightly until you forget how you organized them. Tools like herdr are a big
help here, but even extending it to combine the context with Linear, GitHub, monitoring
did the trick for me. This is how Orbit was created. Context became a click, fixing bugs
became simple tickets with attached Claude sessions and driving new features became multiple
sessions controlled by "the clone".

With this all the unnecessary tab switches, clicks and typing stopped and all the agents
were just working on what I want to work on. This could have been anything from simple bugfixes
to complex feature work and organizing how we can work on this as a team.

As always, as you gain some efficiency there is the point where you have to adjust again since
you became efficient enough that you hit the ceiling again. For me this was "implicit context".

I think we all know that however hard you try, there will never be the full context inside a ticket.
I already gave up on this before, but it represents the underlying issue: "What is in your
brain vs. what's ready for others to be able to pick it up". This alone is not even an article,
it's a research field.

Nonetheless the core issue I had was that too much was still somewhere buried in my thoughts and
not available to be picked up while building. Since the best ideas usually come when talking to
people this was the context I was aiming for. So I built merkwerk. With this I was able
to store all the context from conversations. Being 100% remote helps with this significantly btw.

I explicitly want to have the transcripts since summaries carry the result, but not how we got there.
Just relying on the results would mean that we lose the process of thinking, evaluating and ruling out
options. You might know this challenge with "handovers" between departments (think for example
of design and engineering) where the handing over party shares the result, but it turns out to be critical
to know the thought process of how we got there to make it happen.

This context is what I use for "short-term memory". Just like my memory remembering what I spoke about in the
last couple of days or weeks.

If there is a short-term memory there also needs to be something that fills the gap for example for
long-term priorities: quarterly goals, my personal development goals, results of meetings some months ago.
Since the nature of this context is rather complex, nonlinear and lives from "connecting the dots"
between e.g. company strategy, tech decisions or ideas, I added a simple knowledge graph that is refreshed
every night. It's a simple skill that updates Notion pages with sublinks. Nothing fancy, but enough.

Great. Now we did our best to somehow make our personal context agent readable and allow agents
to use it. But how does this connect to Orbit?

Orbit provides a CLI that allows seeing all the parallel sessions, understanding what we are working on
and plan bigger tasks. All we need to do now is connect. The idea is pretty simple.
Let's create a skill that gathers context from the short- and long-term memory, understands where we are
right now and guides me through what actually matters. It starts with an approx. 10 min research through
merkwerk, Notion and later Slack, GitHub and Orbit and acts as a "chief of staff" that proposes next actions
for tickets and pull requests or even helps with creative tasks like planning user interviews.

To summarize this: We now have a tool that easily allows you to couple the product context with your agents
(Linear <> Claude Code) and exposes a CLI for it to be automated. We also have implicit context of
the thought process and the resulting ideas available locally and the overarching context gathered from
a long-term memory. This is pretty much as good as it gets to just let Claude do my job and me only
become the decision maker.

If you want to learn how we even make it scale across hundreds of cloud agents controlled from this, stay tuned,
I will share something soon... (<- all run on the deepset platform.)

