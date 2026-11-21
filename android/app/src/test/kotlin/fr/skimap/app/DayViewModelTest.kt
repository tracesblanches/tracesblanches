package fr.skimap.app

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import fr.skimap.app.day.ChosenProfile
import fr.skimap.app.day.Contact
import fr.skimap.app.day.DayBackend
import fr.skimap.app.day.DayStore
import fr.skimap.app.day.DayUi
import fr.skimap.app.day.DayViewModel
import fr.skimap.app.day.earliestStart
import fr.skimap.app.day.group
import fr.skimap.core.DayOuting
import fr.skimap.core.DayPlanView
import fr.skimap.core.DayRequest
import fr.skimap.core.DayStart
import fr.skimap.core.GroupSpec
import fr.skimap.core.Lang
import fr.skimap.core.MorningView
import fr.skimap.core.dayDefaultOuting
import fr.skimap.core.groupNew
import fr.skimap.core.groupRead
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import java.io.File
import java.time.LocalDate
import java.time.LocalTime

/** A morning that records what it was asked. */
class FakeDayBackend(
    private val sites: List<Triple<String, String, String>> = listOf(Triple("/packs/chapelle", "chapelle-des-bois", "Chapelle-des-Bois")),
) : DayBackend {
    val requests = ArrayList<Pair<DayRequest, String?>>()
    val validated = ArrayList<Triple<Int, String?, String>>()

    override fun packs(root: File) = sites

    override fun open(
        packDir: String,
        workDir: String,
    ) = object : DayBackend.Planner {
        override fun morning(
            request: DayRequest,
            groupJson: String?,
        ): MorningView {
            requests += request to groupJson
            return DayFixtures.morning
        }

        override fun validate(
            choice: Int,
            groupJson: String?,
            planDir: String,
        ): DayPlanView {
            validated += Triple(choice, groupJson, planDir)
            return DayFixtures.plan.copy(planDir = planDir)
        }
    }

    override fun starts(siteId: String) = listOf(DayFixtures.LAUS, DayStart("Les Rousses", 6.06, 46.49))

    override fun readPlan(planDir: String): DayPlanView = DayFixtures.plan.copy(planDir = planDir)
}

@OptIn(ExperimentalCoroutinesApi::class)
// French phone: the groups and the request labels in French (the core follows the phone).
@RunWith(AndroidJUnit4::class)
@Config(qualifiers = "fr")
class DayViewModelTest {
    @get:Rule
    val tmp = TemporaryFolder()

    @Before
    fun main() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @After
    fun reset() = Dispatchers.resetMain()

    private val ctx = ApplicationProvider.getApplicationContext<Context>()

    private fun store() = DayStore({ ctx.getSharedPreferences("day-test", Context.MODE_PRIVATE) }, { tmp.root })

    private fun vm(
        backend: DayBackend,
        store: DayStore = store(),
    ) = DayViewModel(
        backend,
        store,
        { tmp.root },
        { tmp.newFolder() },
        "chapelle-des-bois",
        LocalDate.of(2026, 11, 16),
        Dispatchers.Unconfined,
        { LocalTime.of(13, 25) },
    )

    @Test
    fun theMorningFollowsTheChoicesAndTheGroup() =
        runTest {
            ctx
                .getSharedPreferences("day-test", Context.MODE_PRIVATE)
                .edit()
                .clear()
                .commit()
            val b = FakeDayBackend()
            val v = vm(b)
            val s = v.state.value as DayUi.Ready
            // Defaults: the first start, 2 h, classic; the family the app offers first, then « Seul ».
            assertEquals(listOf("Famille avec enfants débutants", "Seul"), s.groups.map { it.spec.name })
            assertEquals("Famille avec enfants débutants", s.group!!.spec.name)
            assertEquals(DayFixtures.morning, s.morning)
            assertEquals(1, b.requests.size)
            assertEquals("2 h, classique", b.requests[0].first.label)
            assertEquals(120.0, b.requests[0].first.budgetMin, 0.0)
            // Each choice recomputes the loops.
            v.setInputs { it.copy(budgetMin = 90, skating = true, returnBefore = "16:00", easyOnly = true) }
            val r = b.requests.last().first
            assertEquals("1 h 30, skating, vertes et bleues, retour avant 16 h 00", r.label)
            assertEquals("easy", r.maxDifficulty)
            assertEquals("16:00", r.returnBefore)
            // The group's model goes to the core with the request (C11).
            val other = s.groups[1]
            v.selectGroup(other)
            assertEquals(other.modelJson, b.requests.last().second)
            assertEquals("Seul", store().selectedGroup())
        }

    @Test
    fun theMorningStartsFromTheLastChoiceElseTheCoreDefault() =
        runTest {
            ctx
                .getSharedPreferences("day-test", Context.MODE_PRIVATE)
                .edit()
                .clear()
                .commit()
            // No choice yet: the core's default outing, the one the evening prepares (#170).
            assertNull(store().outing)
            val b = FakeDayBackend()
            vm(b)
            val first = b.requests.last().first
            val default = dayDefaultOuting()
            assertEquals(default.budgetMin, first.budgetMin, 0.0)
            assertEquals(default.skating, first.skating)
            assertEquals(default.returnBefore, first.returnBefore)
            assertNull(first.maxDifficulty)
            // A choice is kept: the next morning and the evening preparation start from it.
            vm(b).setInputs { it.copy(budgetMin = 90, skating = true, returnBefore = "16:00") }
            assertEquals(DayOuting(90.0, true, "16:00", false), store().outing)
            val b2 = FakeDayBackend()
            vm(b2)
            val next = b2.requests.single().first
            assertEquals("1 h 30, skating, retour avant 16 h 00", next.label)
            assertEquals(90.0, next.budgetMin, 0.0)
            assertTrue(next.skating)
        }

    @Test
    fun validateWritesThePlanOfTheChosenLoopForTheGroup() =
        runTest {
            ctx
                .getSharedPreferences("day-test", Context.MODE_PRIVATE)
                .edit()
                .clear()
                .commit()
            val b = FakeDayBackend()
            val st = store()
            val v = vm(b, st)
            v.choose(1)
            v.setContact(Contact("Marie", "0612345678"))
            val p = v.validate().getOrThrow()
            assertEquals(1, b.validated.single().first)
            assertTrue(File(p.planDir).isDirectory)
            assertTrue(p.planDir.endsWith("plans/2026-11-16-1"))
            assertEquals("Famille avec enfants débutants", File(p.planDir, DayStore.GROUP_FILE).readText())
            assertEquals(p, v.validated.value)
            assertEquals(Contact("Marie", "0612345678"), st.contact())
            // A second plan the same day gets its own directory.
            assertTrue(
                v
                    .validate()
                    .getOrThrow()
                    .planDir
                    .endsWith("-2"),
            )
        }

    @Test
    fun aPlanOfTodayStartsAfterNow() {
        val d = LocalDate.of(2026, 11, 15)
        assertEquals("08:00", earliestStart(d.plusDays(1), d, LocalTime.of(13, 25)))
        assertEquals("08:00", earliestStart(d, d, LocalTime.of(6, 30)))
        assertEquals("13:45", earliestStart(d, d, LocalTime.of(13, 25)))
        assertEquals("13:45", earliestStart(d, d, LocalTime.of(13, 35)))
        assertEquals("14:00", earliestStart(d, d, LocalTime.of(13, 36)))
        assertEquals("23:45", earliestStart(d, d, LocalTime.of(23, 50)))
    }

    @Test
    fun noPackOfASupportedSite() =
        runTest {
            val v = vm(FakeDayBackend(emptyList()))
            assertTrue(v.state.value is DayUi.Unavailable)
        }

    @Test
    fun theGroupLearnsFromTheOutingOnce() {
        // Real core (host library): a family model and an observation of one outing.
        ctx
            .getSharedPreferences("day-test", Context.MODE_PRIVATE)
            .edit()
            .clear()
            .commit()
        val st = store()
        val g = groupNew(GroupSpec("Famille test", 2u, byteArrayOf(6, 9), false, "intermediate"), Lang.FR)
        st.putGroup(g)
        val dir = tmp.newFolder("plan")
        st.setPlanGroup(dir.absolutePath, "Famille test")
        File(dir, DayStore.OBSERVATION_FILE).writeText(
            """{"model_s":[1200.0,2400.0,600.0],"moving_s":[2400.0,4100.0,900.0],"progress_m":11000.0,""" +
                """"travelled_m":11900.0,"stops_s":[300.0,420.0,240.0],"elapsed_s":8400.0}""",
        )
        val learned = st.learnFrom(dir.absolutePath)!!
        assertEquals(1u, learned.outings)
        assertEquals("une sortie apprise", learned.learned)
        assertEquals(1u, st.groups().first { it.spec.name == "Famille test" }.outings)
        assertNull(st.learnFrom(dir.absolutePath))
    }

    @Test
    fun theProfileChosenDuringTheOutingLearns() {
        // Real core: the plan was made for the family, the skier chose « Adulte confirmé »
        // during the outing (X14, the core's GroupProfile written by the location service).
        ctx
            .getSharedPreferences("day-test", Context.MODE_PRIVATE)
            .edit()
            .clear()
            .commit()
        val st = store()
        val family = st.groups().first()
        val dir = tmp.newFolder("plan-x14")
        st.setPlanGroup(dir.absolutePath, family.spec.name)
        File(dir, DayStore.PROFILE_CHOSEN_FILE).writeText(
            """{"name":"Adulte confirmé","adults":1,"children":[],"style":"classic","level":"expert"}""",
        )
        File(dir, DayStore.OBSERVATION_FILE).writeText(
            """{"model_s":[1200.0,2400.0,600.0],"moving_s":[1000.0,2000.0,500.0],"progress_m":11000.0,""" +
                """"travelled_m":11300.0,"stops_s":[60.0,0.0,0.0],"elapsed_s":3700.0}""",
        )
        val learned = st.learnFrom(dir.absolutePath)!!
        assertEquals("Adulte confirmé", learned.spec.name)
        assertEquals("expert", learned.spec.level)
        assertEquals(1u, learned.outings)
        // The chosen profile is now a saved group, the plan's group and the one selected.
        assertEquals(1u, st.groups().first { it.spec.name == "Adulte confirmé" }.outings)
        assertEquals(0u, st.groups().first { it.spec.name == family.spec.name }.outings)
        assertEquals("Adulte confirmé", File(dir, DayStore.GROUP_FILE).readText())
        assertEquals("Adulte confirmé", st.selectedGroup())
        assertNull(st.learnFrom(dir.absolutePath))
        // A profile of the same make-up as a saved group is that group.
        val dir2 = tmp.newFolder("plan-x14-2")
        File(dir2, DayStore.PROFILE_CHOSEN_FILE).writeText(
            """{"name":"Adulte","adults":1,"children":[],"style":"classic","level":"intermediate"}""",
        )
        assertEquals("Seul", ChosenProfile.group(st, dir2)?.spec?.name)
    }

    @Test
    fun calibrationVisibleAfterThreeOutings() =
        runTest {
            // V2 (#65), real core: the family the app offers first ends three outings, each
            // learned once at its end; the next morning shows « 3 sorties apprises » and sends
            // the learned model with the request, so the times are the group's.
            ctx
                .getSharedPreferences("day-test", Context.MODE_PRIVATE)
                .edit()
                .clear()
                .commit()
            val st = store()
            val family = st.groups().first()
            assertEquals("a priori, aucune sortie apprise", family.learned)
            val learned =
                (1..3).map { n ->
                    val dir = tmp.newFolder("outing-$n")
                    st.setPlanGroup(dir.absolutePath, family.spec.name)
                    File(dir, DayStore.OBSERVATION_FILE).writeText(
                        """{"model_s":[1200.0,1050.0,750.0],"moving_s":[1800.0,1575.0,1125.0],"progress_m":5400.0,""" +
                            """"travelled_m":5670.0,"stops_s":[300.0,300.0,300.0,300.0],"elapsed_s":5700.0}""",
                    )
                    st.learnFrom(dir.absolutePath)!!.learned
                }
            assertEquals(listOf("une sortie apprise", "2 sorties apprises", "3 sorties apprises"), learned)
            // Learning keeps the group's rank: with no group ever chosen, the group of the
            // next plans (morning, evening preparation, guided outings) is still the family.
            assertEquals(listOf(family.spec.name, "Seul"), st.groups().map { it.spec.name })
            assertNull(st.selectedGroup())
            assertEquals(3u, st.group()!!.outings)
            val b = FakeDayBackend()
            val s = vm(b, st).state.value as DayUi.Ready
            assertEquals(family.spec.name, s.group!!.spec.name)
            assertEquals(3u, s.group.outings)
            assertEquals("3 sorties apprises", s.group.learned)
            val sent = b.requests.single().second!!
            assertEquals(s.group.modelJson, sent)
            assertEquals(3u, groupRead(sent, Lang.FR).outings)
            assertTrue(groupRead(sent, Lang.FR).modelJson != family.modelJson)
        }
}
